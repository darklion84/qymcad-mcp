//! Feature tools: datum planes, extrude, revolve; modifiers (fillet, chamfer, hole, shell, push face); arrays and
//! mirror. Every feature is atomic: if it does not build, nothing changes and QymCAD's reason is returned.

use super::common::{err, rebuild_json, ObjRef, PlaneArg};
use super::topology::{AxisArg, SelArg};
use super::{tool, Tool};
use qymcad_engine::{ArrayDir, Direction, Extrude, Hole, HoleKind, Id, Num, Op, Revolve, Session, Side};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaneOffsetArgs {
    /// A base plane ("XY", "XZ", "YZ") or {"plane": <datum plane>}.
    pub base: PlaneArg,
    /// Distance along the base plane's normal, mm (number or expression).
    pub dist: Num,
    /// Name for the new datum plane (timeline node), usable instead of its id later. Default: QymCAD's generic name.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtrudeArgs {
    /// Sketch id or name.
    pub sketch: ObjRef,
    /// Contour ids from sketch_info. Default: every top-level contour, each minus the contours inside it.
    #[serde(default)]
    pub profiles: Option<Vec<Id>>,
    /// Distance, mm (number or expression). Not needed with `through`.
    #[serde(default)]
    pub height: Option<Num>,
    /// add (default; creates the first body), cut, intersect, new_body.
    #[serde(default)]
    pub op: Op,
    /// normal (default), reverse, symmetric — relative to the sketch plane's normal (XY +Z, XZ −Y, YZ +X, face: outward).
    #[serde(default)]
    pub direction: Direction,
    /// Go through the whole body (cut/add/intersect only).
    #[serde(default)]
    pub through: bool,
    /// Body to modify (id or name); must be a current body, not one consumed by a later feature. Default: the
    /// part's current body. Not allowed with op new_body.
    #[serde(default)]
    pub target: Option<ObjRef>,
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RevolveArgs {
    /// Sketch id or name.
    pub sketch: ObjRef,
    /// Contour ids from sketch_info. Default: every top-level contour, each minus the contours inside it.
    #[serde(default)]
    pub profiles: Option<Vec<Id>>,
    /// The axis to revolve about; must lie in the sketch plane and must not cross the profile: "sketch_x" /
    /// "sketch_y", {"line": <line id of this sketch>}, "X"/"Y"/"Z", {"origin", "dir"}, {"face", "body"?}
    /// (axis of a round face) or {"datum": <id>}.
    pub axis: AxisArg,
    /// Degrees, (0, 360] (number or expression). Default 360.
    #[serde(default)]
    pub angle: Option<Num>,
    /// normal (default): right-hand rule about the axis direction (about sketch_y on XY, +X turns towards −Z); \
    /// reverse; symmetric (half each way).
    #[serde(default)]
    pub direction: Direction,
    /// add (default; creates the first body), cut, intersect, new_body.
    #[serde(default)]
    pub op: Op,
    /// Body to modify (id or name); must be a current body, not one consumed by a later feature. Default: the
    /// part's current body. Not allowed with op new_body.
    #[serde(default)]
    pub target: Option<ObjRef>,
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilletArgs {
    /// Body id or name. Default: the part's current body.
    #[serde(default)]
    pub body: Option<ObjRef>,
    /// The edges to round: ids from `topology` (current body only) or a description, e.g.
    /// {"edges_of": {"facing": "+z"}}. Resolved now and stored by persistent edge names. Seams are not blendable:
    /// the kernel may drop smooth edges, move a seam before blending a neighbouring edge, or refuse the blend.
    pub edges: SelArg,
    /// Radius, mm (number or expression).
    pub radius: Num,
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChamferArgs {
    /// Body id or name. Default: the part's current body.
    #[serde(default)]
    pub body: Option<ObjRef>,
    /// The edges to bevel: ids from `topology` (current body only) or a description, as for fillet.
    pub edges: SelArg,
    /// Setback, mm (number or expression). Alone: a symmetric 45° chamfer.
    pub dist: Num,
    /// Second setback on the other face, mm: an asymmetric chamfer. QymCAD chooses which adjacent face takes
    /// `dist` for each edge; the side cannot be selected.
    #[serde(default)]
    pub d2: Option<Num>,
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HoleArgs {
    /// Body id or name. Default: the part's current body.
    #[serde(default)]
    pub body: Option<ObjRef>,
    /// The planar face to drill into; must match exactly one face, e.g. {"facing": "+z"} or a face id. The hole
    /// goes into the body, against the face's outward normal.
    pub face: SelArg,
    /// Hole centre in WORLD coordinates [x, y, z], mm; projected onto the face along its normal, so only the
    /// in-plane position matters. Default: the face centroid. Plain numbers (QymCAD does not store a hole's
    /// position parametrically; for parametric hole patterns cut sketch circles with extrude instead).
    #[serde(default)]
    pub at: Option<[f64; 3]>,
    /// Diameter, mm (number or expression).
    pub diameter: Num,
    /// Depth from the face, mm (number or expression). Give `depth` or `through`.
    #[serde(default)]
    pub depth: Option<Num>,
    /// Stored as a fixed 10000 mm depth (the app's own maximum; QymCAD holes have no through-all). Creation is
    /// refused if the body's bbox diagonal exceeds 10000 mm. Growing the stock beyond this depth can make it blind.
    #[serde(default)]
    pub through: bool,
    /// plain (default), counterbore (dia2 × depth2 at the face), countersink (cone from dia2 at the face down to
    /// the diameter over depth2).
    #[serde(default)]
    pub kind: HoleKind,
    /// Counterbore/countersink only: diameter at the face, mm (number or expression); must exceed `diameter`.
    #[serde(default)]
    pub dia2: Option<Num>,
    /// Counterbore/countersink only: depth of the recess, mm (number or expression); counterbore = depth of
    /// the wide cylinder, countersink = depth of the cone. Must be less than the hole depth.
    #[serde(default)]
    pub depth2: Option<Num>,
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShellArgs {
    /// Body id or name. Default: the part's current body.
    #[serde(default)]
    pub body: Option<ObjRef>,
    /// Faces to remove (the openings), at least one, e.g. {"facing": "+z"} for an open box. QymCAD has no closed
    /// hollow shell.
    pub open_faces: SelArg,
    /// Wall thickness, mm (number or expression).
    pub thickness: Num,
    /// inward (default: the outside stays), outward (the body becomes the cavity), centred.
    #[serde(default)]
    pub side: Side,
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PushFaceArgs {
    /// Body id or name. Default: the part's current body.
    #[serde(default)]
    pub body: Option<ObjRef>,
    /// One planar face, e.g. {"facing": "+z"}.
    pub face: SelArg,
    /// Distance along the outward normal, mm (number or expression); negative pulls the face in.
    pub dist: Num,
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name.
    #[serde(default)]
    pub name: Option<String>,
}

fn zero() -> Num {
    Num::Value(0.0)
}

/// One direction of a linear array.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArrayDirArg {
    /// Step between copies, mm (numbers or expressions; default 0).
    #[serde(default = "zero")]
    pub dx: Num,
    /// Step along Y between copies, mm (number or expression; default 0).
    #[serde(default = "zero")]
    pub dy: Num,
    /// Step along Z between copies, mm (number or expression; default 0).
    #[serde(default = "zero")]
    pub dz: Num,
    /// Number of copies including the original (number or expression). At most 1000 copies per array in total
    /// (all directions multiplied), also when a parameter edit changes it.
    pub count: Num,
}

impl From<ArrayDirArg> for ArrayDir {
    fn from(a: ArrayDirArg) -> Self {
        ArrayDir { dx: a.dx, dy: a.dy, dz: a.dz, count: a.count }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinearArrayArgs {
    /// Body id or name. Default: the part's current body.
    #[serde(default)]
    pub body: Option<ObjRef>,
    /// Step between copies, mm (numbers or expressions; default 0).
    #[serde(default = "zero")]
    pub dx: Num,
    /// Step along Y between copies, mm (number or expression; default 0).
    #[serde(default = "zero")]
    pub dy: Num,
    /// Step along Z between copies, mm (number or expression; default 0).
    #[serde(default = "zero")]
    pub dz: Num,
    /// Number of copies including the original (number or expression). At most 1000 copies per array in total
    /// (all directions multiplied), also when a parameter edit changes it.
    pub count: Num,
    /// A second direction, for a grid.
    #[serde(default)]
    pub second: Option<ArrayDirArg>,
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name.
    #[serde(default)]
    pub name: Option<String>,
}

fn full_turn() -> Num {
    Num::Value(360.0)
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CircularArrayArgs {
    /// Body id or name. Default: the part's current body.
    #[serde(default)]
    pub body: Option<ObjRef>,
    /// Number of copies including the original (number or expression). At most 1000 copies per array in total
    /// (all directions multiplied), also when a parameter edit changes it.
    pub count: Num,
    /// Degrees, (0, 360] (default 360). An angle of 359.9° or more counts as a full turn: copies 360/count apart.
    /// A smaller angle spaces them angle/count apart, so the last copy is at angle·(count−1)/count (QymCAD's rule).
    #[serde(default = "full_turn")]
    pub angle: Num,
    /// Default: world Z through the origin.
    #[serde(default)]
    pub axis: Option<AxisArg>,
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name.
    #[serde(default)]
    pub name: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MirrorArgs {
    /// Body id or name. Default: the part's current body.
    #[serde(default)]
    pub body: Option<ObjRef>,
    /// "XY", "XZ", "YZ", {"plane": <datum plane>} or {"body": <body>, "face": <planar face id>}.
    pub plane: PlaneArg,
    /// Keep the original as well (default true): one body with both halves. false: only the image.
    #[serde(default = "yes")]
    pub keep: bool,
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name.
    #[serde(default)]
    pub name: Option<String>,
}

fn opt_body(s: &Session, b: &Option<ObjRef>) -> Result<Option<Id>, String> {
    b.as_ref().map(|b| b.resolve(s)).transpose()
}

fn feature_json(r: Result<(Id, qymcad_engine::Rebuild), qymcad_engine::Error>) -> Result<serde_json::Value, String> {
    let (id, r) = r.map_err(err)?;
    Ok(json!({ "body": id, "rebuild": rebuild_json(&r) }))
}

/// Appended to every feature description.
macro_rules! stale {
    () => {
        " Returns the new body id (the part's current body from now on); face/edge ids read from earlier bodies are \
         stale — call topology again before picking more."
    };
}

pub fn tools() -> Vec<Tool> {
    vec![
        tool(
            "plane_offset",
            "Create a datum plane parallel to a base or datum plane at a distance (e.g. the top face level for a pocket: \
             {\"base\": \"XY\", \"dist\": \"t\"}). Returns its id; sketch on it with {\"plane\": id}.",
            |st, a: PlaneOffsetArgs| {
                let s = st.doc()?;
                let base = a.base.resolve(s)?;
                let (id, r) = s.plane_offset(&base, &a.dist, a.name.as_deref()).map_err(err)?;
                Ok(json!({ "plane": id, "rebuild": rebuild_json(&r) }))
            },
        ),
        tool(
            "extrude",
            concat!(
                "Extrude sketch contours: add material (the first add creates the part's body), cut, intersect, or a \
                 new body. Atomic: if the feature does not build, nothing changes and the error is returned. Also \
                 returns the result bodies (volume, bbox). One-sided cuts extend the entry end 0.001 mm behind the sketch plane to break coplanarity. \
                 On an internal datum plane this removes an extra 0.001 mm of material; a cut entering from a stock face keeps its nominal pocket depth. \
                 Cuts ending at the far stock boundary also receive clearance to become through cuts; through=true spans the whole stock.",
                stale!()
            ),
            |st, a: ExtrudeArgs| {
                let s = st.doc()?;
                let sketch = a.sketch.resolve(s)?;
                let target = match &a.target {
                    Some(t) => Some(t.resolve(s)?),
                    None => None,
                };
                let height = match (a.height, a.through) {
                    (Some(h), _) => h,
                    (None, true) => Num::Value(1.0),
                    (None, false) => return Err("`height` is required unless `through` is set".into()),
                };
                let x = Extrude {
                    sketch,
                    profiles: a.profiles,
                    height,
                    op: a.op,
                    direction: a.direction,
                    through: a.through,
                    target,
                    name: a.name,
                };
                let (id, r) = s.extrude(&x).map_err(err)?;
                Ok(json!({ "body": id, "rebuild": rebuild_json(&r) }))
            },
        ),
        tool(
            "revolve",
            concat!(
                "Revolve sketch contours about an axis in the sketch plane: add material (the first add creates the \
             part's body), cut, intersect, or a new body. Atomic.",
                stale!()
            ),
            |st, a: RevolveArgs| {
                let s = st.doc()?;
                let x = Revolve {
                    sketch: a.sketch.resolve(s)?,
                    profiles: a.profiles,
                    axis: a.axis.resolve(s)?,
                    angle: a.angle.unwrap_or(Num::Value(360.0)),
                    direction: a.direction,
                    op: a.op,
                    target: opt_body(s, &a.target)?,
                    name: a.name,
                };
                feature_json(s.revolve(&x))
            },
        ),
        tool(
            "fillet",
            concat!(
                "Round edges of a body. `edges` is a selection: ids from topology, or a description such as \
             {\"edges_of\": {\"facing\": \"+z\"}} (top outline), {\"along\": \"z\"} (vertical edges), or \
             {\"concave\": true} (inward corners, including boss/plate circles). For a named boss/plate junction \
             use {\"between\": [{\"of_feature\": \"X\", \"role\": \"wall\"}, {\"of_feature\": \"Y\", \"role\": \"cap_end\"}]}. \
             Concave/convex exclude seams and G1 tangent junctions; uncertain shallow signs or disagreement \
             among five arc-length samples are omitted. Analytic cone rims at slopes >=3 degrees are tested; \
             0.9 degrees may be omitted (native smoothness threshold about 1.5 degrees). Fallback facet normals \
             can require about 20–30 degrees. The edges \
             are stored by their persistent names, which QymCAD carries across upstream edits. A radius too big \
             for the geometry is an error and nothing changes. During blending, the kernel extends a blend along edges tangent-continuous with a selected edge; preview with select shows only the selected edges.",
                stale!()
            ),
            |st, a: FilletArgs| {
                let s = st.doc()?;
                let (body, edges) = (opt_body(s, &a.body)?, a.edges.resolve(s)?);
                feature_json(s.fillet(body, &edges, &a.radius, a.name.as_deref()))
            },
        ),
        tool(
            "chamfer",
            concat!(
                "Bevel edges of a body: `dist` alone is symmetric; with `d2` the two setbacks differ. `edges` as for \
             fillet. During blending, the kernel extends a blend along edges tangent-continuous with a selected edge; preview with select shows only the selected edges.",
                stale!()
            ),
            |st, a: ChamferArgs| {
                let s = st.doc()?;
                let (body, edges) = (opt_body(s, &a.body)?, a.edges.resolve(s)?);
                feature_json(s.chamfer(body, &edges, &a.dist, a.d2.as_ref(), a.name.as_deref()))
            },
        ),
        tool(
            "hole",
            concat!(
                "Drill a hole into a planar face: plain, counterbore or countersink; blind (`depth`) or `through`. \
             The face may be described ({\"facing\": \"+z\"}, {\"of_feature\": \"plate\", \"role\": \"cap_end\"}): \
             such a description is stored and keeps working after upstream edits. `at` is a world point. \
             `through` stores a fixed 10000 mm depth; creation is refused when the body's bbox diagonal exceeds \
             10000 mm. Later stock growth beyond that depth can make the hole blind.",
                stale!()
            ),
            |st, a: HoleArgs| {
                let s = st.doc()?;
                let depth = match (a.depth, a.through) {
                    (Some(_), true) => return Err("give `depth` or `through`, not both".into()),
                    (None, false) => return Err("give `depth` or `through: true`".into()),
                    (d, _) => d,
                };
                let h = Hole {
                    body: opt_body(s, &a.body)?,
                    face: a.face.resolve(s)?,
                    at: a.at,
                    diameter: a.diameter,
                    depth,
                    kind: a.kind,
                    dia2: a.dia2,
                    depth2: a.depth2,
                    name: a.name,
                };
                feature_json(s.hole(&h))
            },
        ),
        tool(
            "shell",
            concat!(
                "Hollow a body with walls of `thickness`, removing `open_faces` (at least one; e.g. {\"facing\": \"+z\"} for an open box).",
                stale!()
            ),
            |st, a: ShellArgs| {
                let s = st.doc()?;
                let body = opt_body(s, &a.body)?;
                let faces = a.open_faces.resolve(s)?;
                feature_json(s.shell(body, &faces, &a.thickness, a.side, a.name.as_deref()))
            },
        ),
        tool(
            "push_face",
            concat!("Move one planar face along its outward normal by `dist` (negative = into the body), e.g. raise the top.", stale!()),
            |st, a: PushFaceArgs| {
                let s = st.doc()?;
                let (body, face) = (opt_body(s, &a.body)?, a.face.resolve(s)?);
                feature_json(s.push_face(body, &face, &a.dist, a.name.as_deref()))
            },
        ),
        tool(
            "linear_array",
            concat!(
                "Copy the WHOLE body `count` times along (dx, dy, dz), optionally also along a `second` direction (a \
             grid). The result is one body holding every copy, including all existing holes and cuts. QymCAD has no single-feature \
             pattern. For repeated holes, add circles at expression-driven centres in one sketch, then extrude with op=cut and \
             through=true; this repeats the cut without copying the stock.",
                stale!()
            ),
            |st, a: LinearArrayArgs| {
                let s = st.doc()?;
                let body = opt_body(s, &a.body)?;
                let d1 = ArrayDir { dx: a.dx, dy: a.dy, dz: a.dz, count: a.count };
                let d2: Option<ArrayDir> = a.second.map(Into::into);
                feature_json(s.linear_array(body, &d1, d2.as_ref(), a.name.as_deref()))
            },
        ),
        tool(
            "circular_array",
            concat!(
                "Copy the WHOLE body `count` times about an axis (default world Z), including all existing holes and cuts. \
                 The result is one body. QymCAD has no single-feature pattern. For repeated holes, add circles at \
                 expression-driven centres in one sketch, then extrude with op=cut and through=true; this repeats the cut \
                 without copying the stock.",
                stale!()
            ),
            |st, a: CircularArrayArgs| {
                let s = st.doc()?;
                let body = opt_body(s, &a.body)?;
                let axis = a.axis.as_ref().map(|x| x.resolve(s)).transpose()?;
                feature_json(s.circular_array(body, &a.count, &a.angle, axis.as_ref(), a.name.as_deref()))
            },
        ),
        tool(
            "mirror",
            concat!(
                "Mirror the whole body about a base plane, a datum plane or a planar face; `keep` (default true) keeps \
             the original too, as one body.",
                stale!()
            ),
            |st, a: MirrorArgs| {
                let s = st.doc()?;
                let body = opt_body(s, &a.body)?;
                let plane = a.plane.resolve(s)?;
                feature_json(s.mirror(body, &plane, a.keep, a.name.as_deref()))
            },
        ),
    ]
}
