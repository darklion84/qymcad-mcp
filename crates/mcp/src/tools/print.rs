//! Printing feature tools. Native recipe edits and geometric validation live in the engine.
use super::common::{err, rebuild_json, ObjRef};
use super::topology::SelArg;
use super::{tool, Tool};
use qymcad_engine::{
    BaseName, BodyBooleanOptions, BooleanOp, Element, Num, PlaneRef, Session, SplitFaceOptions, SplitOptions, ThreadOptions,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ThreadArgs {
    /// Source body id or name. Default: the current unconsumed body.
    #[serde(default)]
    body: Option<ObjRef>,
    /// Exactly one circular rim from this body's current topology: an id or an edge selection. Resolved once and stored as a persistent rim id.
    edge: SelArg,
    /// Size alias: M8, M8x1.25 or Tr16x4. Excludes standard, nominal and pitch; never supplies length.
    #[serde(default)]
    size: Option<String>,
    /// Native standard: metric_iso (default), trapezoidal_tr, acme, round_rd or custom. buttress is refused: native builds it as a symmetric 52 degree profile. Excludes size.
    #[serde(default)]
    standard: Option<String>,
    /// Basic nominal diameter, mm (number or expression). Required unless size is supplied.
    #[serde(default)]
    nominal: Option<Num>,
    /// Pitch, mm (number or expression). Default 0 selects the standard's coarse pitch. Excludes size.
    #[serde(default)]
    pitch: Option<Num>,
    /// Required thread length, mm (number or expression), within the selected cylindrical stock span.
    length: Num,
    /// auto (default) detects the material side; external or internal asserts that the detected side agrees.
    #[serde(default = "auto")]
    kind: String,
    /// Modelling fit clearance, mm per side (number or expression). Default 0.2 is lowered to keep radial relief at most 0.13 pitch; explicit values beyond that limit are refused after every edit too. Result reports the fit and reason; no ISO tolerance class is implied.
    #[serde(default)]
    fit: Option<Num>,
    /// Entry runout length, mm (number or expression). App default 1.5. 0, or above 1e-6; lead_in + lead_out at most 0.95 of length (native would drop or scale them).
    #[serde(default)]
    lead_in: Option<Num>,
    /// Exit runout length, mm (number or expression). App default 1.5. Same limits as lead_in.
    #[serde(default)]
    lead_out: Option<Num>,
    /// Number of starts (number or expression): an integer 1..64, checked again after every edit. Default 1.
    #[serde(default)]
    starts: Option<Num>,
    /// Left-hand thread when true. Default false is right-hand.
    #[serde(default)]
    left: bool,
    /// Crest rounding radius, mm (number or expression). Default 0 uses the standard profile. Explicit radii must appear on their own crest/root arcs of the actual stock profile; clamped or missing arcs are refused.
    #[serde(default)]
    crest_r: Option<Num>,
    /// Root rounding radius, mm (number or expression). Default 0 uses the standard profile. Explicit radii must appear on their own crest/root arcs of the actual stock profile; clamped or missing arcs are refused.
    #[serde(default)]
    root_r: Option<Num>,
    /// Included flank angle, custom profile only, degrees (number or expression): greater than 1 and at most 148.969 (native ignores angles at or below 1 and clamps the half-angle at 1.3 rad). Default 60.
    #[serde(default)]
    angle: Option<Num>,
    /// Radial depth, custom profile only, mm (number or expression). Default 0 uses the native pitch-based depth. Refused when the profile cannot hold it (the flank angle would change) or when depth plus fit is so shallow that native would cut on the wrong side.
    #[serde(default)]
    depth: Option<Num>,
    /// Name of the new thread feature/body, usable instead of its id later. Names interpreted as numeric ids are refused.
    #[serde(default)]
    name: Option<String>,
}

fn auto() -> String {
    "auto".into()
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BodyBooleanArgs {
    /// Current base body A, by id or name. Must be distinct from B and in the same part.
    a: ObjRef,
    /// Current tool body B, by id or name. Both inputs are consumed by the result.
    b: ObjRef,
    /// cut (default, A minus B), union, or intersect. Cut and intersect may return multiple separately addressable bodies.
    #[serde(default = "default_boolean_op")]
    op: String,
    /// Timeline feature name; numeric-looking names are refused.
    #[serde(default)]
    name: Option<String>,
    /// Optional body names for every output piece, primary first. Names are returned and can be used by later tools. Later count/solid changes or ambiguous piece identities are refused atomically.
    #[serde(default)]
    piece_names: Option<Vec<String>>,
}

fn default_boolean_op() -> String {
    "cut".into()
}

/// Split operation plane: its XZ offset direction follows the native operation's +Y normal.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
enum SplitPlaneArg {
    Base(BaseName),
    Datum {
        /// Datum plane id or name.
        plane: ObjRef,
    },
    Face {
        /// Body id or name containing the reference face.
        body: ObjRef,
        /// Exactly one planar face id or selection, e.g. {"facing":"+z"}.
        face: SelArg,
    },
}

impl SplitPlaneArg {
    fn resolve(&self, session: &mut Session) -> Result<PlaneRef, String> {
        Ok(match self {
            Self::Base(base) => PlaneRef::Base(*base),
            Self::Datum { plane } => PlaneRef::Plane(plane.resolve(session)?),
            Self::Face { body, face } => {
                let body = body.resolve(session)?;
                let selection = face.resolve(session)?;
                let (_, faces) = session.select(Some(body), Element::Faces, &selection).map_err(err)?;
                if faces.len() != 1 {
                    return Err(format!("split plane must select exactly one planar face; matched {}", faces.len()));
                }
                PlaneRef::Face { body, face: faces[0] }
            }
        })
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SplitArgs {
    /// Source body id or name. Default: the current unconsumed body. Use an explicit body after a split.
    #[serde(default)]
    body: Option<ObjRef>,
    /// Cutting plane: XY (+Z), XZ (+Y), YZ (+X), {"plane":<datum id/name>} or {"body":<id/name>,"face":<one planar face selection>}.
    plane: SplitPlaneArg,
    /// Offset in mm along the operation plane's normal (number or expression). Default 0; XZ points along +Y.
    #[serde(default = "zero")]
    offset: Num,
    /// Split feature name, also used as the prefix for default native piece names. Names interpreted as numeric ids are refused.
    #[serde(default)]
    name: Option<String>,
    /// Distinct native names of every resulting piece, in result order. Names interpreted as numeric ids (including +1 or leading zeroes) are refused. Default: <feature name> plus the next unused positive numbers in the document (1, 2, etc.), so successive unnamed splits stay addressable.
    #[serde(default)]
    piece_names: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SplitFaceArgs {
    /// Body to cut, by id or name. Default: the current unconsumed body of the active Part.
    #[serde(default)]
    body: Option<ObjRef>,
    /// Cutting plane: XY (+Z), XZ (+Y), YZ (+X), {"plane":<datum id/name>} or {"body":<id/name>,"face":<one PLANAR face selection>}. Base planes and offsets are in the local frame of the body's Part, not the world.
    plane: SplitPlaneArg,
    /// Offset in mm along the plane's normal (number or expression). Default 0; XZ points along +Y. A formula stays on the node and follows its parameters.
    #[serde(default = "zero")]
    offset: Num,
    /// Name of the Split faces feature; names interpreted as numeric ids are refused.
    #[serde(default)]
    name: Option<String>,
}

fn zero() -> Num {
    Num::Value(0.0)
}

/// Register the native printing feature tools.
pub(super) fn tools() -> Vec<Tool> {
    vec![tool(
        "thread",
        "Cut a native parametric thread from exactly one circular rim of the current stock. Use size M8/M8x1.25/Tr16x4 or canonical standard/nominal/pitch; length is required. The engine detects external/internal material and refuses an explicit kind mismatch, off-size stock, overlong threads and cuts that remove nothing. App print defaults are lead_in/lead_out 1.5 mm and fit 0.2 mm, lowered when necessary so radial relief stays at most 0.13 pitch. ISO internal bores also need diameter at least D1-.26*pitch+2*effective_relief (D1=nominal-1.082532*pitch; effective_relief is radial relief above 1e-6 mm, otherwise zero); external stock remains within nominal +/- max(.05*nominal,.5*pitch). Stock and profile edits recheck the 0.02 pitch groove exposure after regeneration. Custom angle must be >1 and at most 148.969 degrees (half-angle at most 1.3 radians). Explicit fits above the relief limit and later unsafe edits are refused; the result reports the fit used and reason. Every numeric field remains editable through dimension_set's feature/key target; numeric strings retain the same full precision as JSON numbers. Returns the new body id, persistent rim, detected side, effective pitch/lead, standard basic diameters and fit radial relief, plus the normal rebuild report. For internal threads, basic_major_diameter is the reference root diameter, basic_minor_diameter the reference crest diameter, and basic_pitch_diameter the reference pitch cylinder. Tr includes ISO root clearance (Tr16x4 internal: major 16.5, minor 12); Rd includes DIN 405 internal crest/root clearance. ACME reports the basic profile without native cutter backlash. Custom reports nominal, nominal-depth, nominal-2*depth on either side. These are standard/design references, not measured fit-adjusted cavity limits; fit is modelling clearance, not an ISO tolerance class or a printed mating guarantee. Atomic on rebuild failure. Read topology again before selecting edges on the resulting body.",
        |st, a: ThreadArgs| {
            let session = st.doc()?;
            let body = a.body.as_ref().map(|body| body.resolve(session)).transpose()?;
            let edge = a.edge.resolve(session)?;
            let options = ThreadOptions { size: a.size, standard: a.standard, nominal: a.nominal, pitch: a.pitch, length: a.length, kind: a.kind,
                fit: a.fit, lead_in: a.lead_in, lead_out: a.lead_out, starts: a.starts, left: a.left, crest_r: a.crest_r,
                root_r: a.root_r, angle: a.angle, depth: a.depth, name: a.name };
            let result = session.thread(body, &edge, &options).map_err(err)?;
            let mut value = json!(&result);
            value["rebuild"] = rebuild_json(&result.rebuild);
            Ok(value)
        },
    ), tool(
        "body_boolean",
        "Apply a native parametric boolean between two distinct current bodies in the same part. Both operands are consumed; the node is created in their owning part without changing the active part. cut computes A minus B; union and intersect are explicit. Refuses same/stale bodies, no-op cuts, empty intersections and rebuild errors atomically. Cut/intersect may create multiple named bodies; union remains one body even when it contains multiple solids. Output names are reported and resolvable; omitted names use the next unused <feature name> N suffix. Later changes in output body count, solid count or ambiguous piece identity are refused atomically; recreate the boolean for topology changes. Returns the feature/body, operation, consumed ids, each output body's volume/bounds and rebuild diagnostics.",
        |st, a: BodyBooleanArgs| {
            let session = st.doc()?;
            let a_id = a.a.resolve(session)?;
            let b_id = a.b.resolve(session)?;
            let op = match a.op.as_str() { "cut" => BooleanOp::Cut, "union" => BooleanOp::Union, "intersect" => BooleanOp::Intersect,
                _ => return Err("op must be cut, union or intersect".into()) };
            let result = session.body_boolean(&BodyBooleanOptions { a: a_id, b: b_id, op, name: a.name, piece_names: a.piece_names }).map_err(err)?;
            let mut value = json!(&result);
            value["rebuild"] = rebuild_json(&result.rebuild);
            Ok(value)
        },
    ), tool(
        "split",
        "Split one body into separate bodies using a base plane, datum plane or planar face and a formula-capable offset. Base-plane offset directions are XY +Z, XZ +Y and YZ +X. Refuses a plane that cuts nothing. The piece count is fixed at creation; later edits that change it fail atomically. Every rebuilt split must retain piece identity by unique maximum total bounding-box overlap; swaps, ambiguous matches and moves without overlap are refused atomically with affected piece names. Recreate the split for such changes. Returns the feature/body id and every piece's native id, name, volume and bounds. Names default to <feature name> 1..n or use piece_names; names interpreted as numeric ids are refused. Use an explicit body after a split for further modelling; omitting body retains the usual current-tip behavior. Numeric ids and native piece names select any piece in modelling tools and export bodies; export writes one file per call, and bodies:[piece] writes that piece alone.",
        |st, a: SplitArgs| {
            let session = st.doc()?;
            let body = a.body.as_ref().map(|body| body.resolve(session)).transpose()?;
            let plane = a.plane.resolve(session)?;
            let result = session.split(&SplitOptions { body, plane, offset: a.offset, name: a.name, piece_names: a.piece_names }).map_err(err)?;
            let mut value = json!(&result);
            value["rebuild"] = rebuild_json(&result.rebuild);
            Ok(value)
        },
    ), tool(
        "split_face",
        "Split faces, as the app's Split faces command: cut the faces of a body along a plane and keep the body in ONE piece. Nothing is removed and the volume does not change; the result has more faces, so that a later tool can pick a part of a wall (push_face, draft, shell, hole, thicken, remove_face, a datum or a sketch on it). This is not `split`, which cuts the material into separate bodies. The plane is a base plane (XY +Z, XZ +Y, YZ +X), a datum plane or one planar face, shifted by `offset` (number or formula, along the plane's normal); it is an infinite plane and cuts EVERY face it crosses, curved faces and the walls of holes included; there is no choice of faces. Base planes and offsets are in the local frame of the body's Part (the frame the rebuild cuts in; the app's own pre-check measures in the world frame, F-194), so for a placed Part read the body's bounds in doc_info. A plane taken from a face follows that face when it moves. One action: one timeline node, atomic, one undo step, allowed in batch. Returns `feature` (the node; also the body's id), `body`, `name`, the evaluated `offset`, `faces` {before, after}, `added_faces` (ids the result has and the source did not) and the rebuild report. The cut face keeps its first piece's id; the other pieces get fresh ids, so read topology (or use select with the added ids) after the call to choose a piece: the pieces of one face share a normal and are told apart by centroid or position. Behaviour of the app, accepted: suppressing a Split faces node gives its consumers (a push_face on a piece, for instance) the WHOLE face instead of the piece, and unsuppressing returns the pieces (F-192); a fillet on the edge of one piece rounds the whole edge chain, not only that piece's part (F-193). Refused, each with a reason and the document unchanged: a plane that splits nothing or misses the body; a curved face as the plane (F-191); a plane within about 1e-5 mm of a face or through a torus's axis, where the app reports success but splits nothing and can damage the body (F-187); a result whose face ids are not unique (a second split that crosses the first, or runs parallel to it on the wrong side: below it along +Z or +Y, above it along +X, F-189); a plane taken from a face that later disappears (F-190); a later edit (param_set, dimension_set, an edit upstream, reorder) that changes which faces the node cuts, because the old ids would then silently point at other pieces (F-188: recreate the split); a non-finite offset; no body, a mesh body, a suppressed body, a clone made by part_instance, or an assembly as the active context.",
        |st, a: SplitFaceArgs| {
            let session = st.doc()?;
            let body = a.body.as_ref().map(|body| body.resolve(session)).transpose()?;
            let plane = a.plane.resolve(session)?;
            let result = session.split_face(&SplitFaceOptions { body, plane, offset: a.offset, name: a.name }).map_err(err)?;
            let mut value = json!(&result);
            value["rebuild"] = rebuild_json(&result.rebuild);
            Ok(value)
        },
    )]
}
