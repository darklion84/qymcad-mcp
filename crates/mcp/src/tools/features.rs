//! Feature tools: datum planes, extrude, revolve, sweep, loft; modifiers (fillet, chamfer, hole, shell, push face); arrays and
//! mirror. Every feature is atomic: if it does not build, nothing changes and QymCAD's reason is returned.

use super::common::{err, rebuild_json, ObjRef, PlaneArg};
use super::topology::{AxisArg, SelArg};
use super::{tool, Tool};
use qymcad_engine::{
    ArrayDir, Direction, Extrude, Hole, HoleKind, Id, Loft, LoftSection, Num, Op, Revolve, Session, Side, Sweep, SweepPath,
};
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
    /// Name for the new datum plane (timeline node), usable instead of its id later. Default: QymCAD's generic name. Names interpreted as numeric ids are refused.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtrudeArgs {
    /// Sketch id or name.
    pub sketch: ObjRef,
    /// Contour ids from sketch_info. Default: every top-level contour, including all text glyph outlines with counters kept as holes.
    #[serde(default)]
    pub profiles: Option<Vec<Id>>,
    /// Pick contours by points [[x, y], ...] in sketch coordinates instead of ids: each point takes the innermost
    /// closed contour around it, as a click in the app does. Evaluated once. Excludes `profiles`.
    #[serde(default)]
    pub profiles_at: Option<Vec<[Num; 2]>>,
    /// Distance, mm (number or expression). Not needed with `through`.
    #[serde(default)]
    pub height: Option<Num>,
    /// Second-side length in mm (number or expression), finite and greater than 1e-9. Gives native Two sides: normal spans [-down, height], reverse [-height, down]. Excludes symmetric and through; later parameter/dimension edits must keep it positive. Omit for one side.
    #[serde(default)]
    pub down: Option<Num>,
    /// add (default; creates the first body), cut, intersect, new_body.
    #[serde(default)]
    pub op: Op,
    /// normal (default), reverse, symmetric — relative to the sketch plane's normal (XY +Z, XZ −Y, YZ +X, face: outward).
    #[serde(default)]
    pub direction: Direction,
    /// Go through the whole body to the chosen side (cut/add/intersect only): `normal` or `reverse` reach the
    /// stock boundary on that side; `symmetric` goes through on both sides of the sketch plane.
    #[serde(default)]
    pub through: bool,
    /// Body to modify (id or name); must be a current body, not one consumed by a later feature. Default: the
    /// part's current body. Not allowed with op new_body.
    #[serde(default)]
    pub target: Option<ObjRef>,
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name. Names interpreted as numeric ids are refused.
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
    /// Pick contours by points [[x, y], ...] in sketch coordinates instead of ids: each point takes the innermost
    /// closed contour around it, as a click in the app does. Evaluated once. Excludes `profiles`.
    #[serde(default)]
    pub profiles_at: Option<Vec<[Num; 2]>>,
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
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name. Names interpreted as numeric ids are refused.
    #[serde(default)]
    pub name: Option<String>,
}

/// The path of a sweep: a contour of another sketch, or a chain of body edges.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PathArg {
    /// A contour of another sketch, open or closed.
    Sketch(SketchPathArg),
    /// Edges of a body joining end to end into one chain.
    Edges(EdgePathArg),
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SketchPathArg {
    /// Path sketch id or name (not the profile sketch).
    pub sketch: ObjRef,
    /// Path contour id from sketch_info; required when the sketch has more than one.
    #[serde(default)]
    pub profile: Option<Id>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EdgePathArg {
    /// Body id or name. Default: the part's current body.
    #[serde(default)]
    pub body: Option<ObjRef>,
    /// The edges: ids from topology or a description; stored by their persistent names.
    pub edges: SelArg,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SweepArgs {
    /// Profile sketch id or name.
    pub sketch: ObjRef,
    /// Closed profile contour ids. Default: every top-level closed contour.
    #[serde(default)]
    pub profiles: Option<Vec<Id>>,
    /// Pick profile contours by points [[x, y], ...] in sketch coordinates instead of ids. Excludes `profiles`.
    #[serde(default)]
    pub profiles_at: Option<Vec<[Num; 2]>>,
    /// {"sketch": <path sketch>, "profile"?: <contour id>} or {"body"?: <body>, "edges": <selection>}.
    pub path: PathArg,
    /// add (default; creates the first body), cut, intersect, new_body.
    #[serde(default)]
    pub op: Op,
    /// Body to modify (id or name). Default: the part's current body. Not allowed with op new_body.
    #[serde(default)]
    pub target: Option<ObjRef>,
    /// Name for the new feature (timeline node), usable instead of its id later. Names interpreted as numeric ids are refused.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SectionArg {
    /// Section sketch id or name.
    pub sketch: ObjRef,
    /// Its closed contour id; required when the sketch has more than one. Excludes `profile_at`.
    #[serde(default)]
    pub profile: Option<Id>,
    /// Pick the contour by a point [x, y] inside it, in sketch coordinates. Excludes `profile`.
    #[serde(default)]
    pub profile_at: Option<[Num; 2]>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoftArgs {
    /// At least two sections, in order; usually sketches on parallel datum planes.
    pub sections: Vec<SectionArg>,
    /// Straight faces between sections (default false: smooth, the app's default).
    #[serde(default)]
    pub ruled: bool,
    /// add (default; creates the first body), cut, intersect, new_body.
    #[serde(default)]
    pub op: Op,
    /// Body to modify (id or name). Default: the part's current body. Not allowed with op new_body.
    #[serde(default)]
    pub target: Option<ObjRef>,
    /// A sheet through the sections instead of a solid; only with op new_body.
    #[serde(default)]
    pub surface: bool,
    /// Name for the new feature (timeline node), usable instead of its id later. Names interpreted as numeric ids are refused.
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
    /// {"edges_of": {"facing": "+z"}}. Resolved now and stored by persistent edge names. Seam edges are
    /// automatically dropped with a result note; a selection containing only seams is refused.
    pub edges: SelArg,
    /// Radius, mm (number or expression).
    pub radius: Num,
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name. Names interpreted as numeric ids are refused.
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
    /// Seam edges are automatically dropped with a result note; a selection containing only seams is refused.
    pub edges: SelArg,
    /// Setback, mm (number or expression). Alone: a symmetric 45° chamfer.
    pub dist: Num,
    /// Second setback on the other face, mm: an asymmetric chamfer. QymCAD chooses which adjacent face takes
    /// `dist` for each edge by its native traversal order; the side cannot be selected or reported. Inspect
    /// adjacent face setbacks before/after chamfer; for straight rectangular faces, lost area / edge length
    /// gives the setback. Topology's sorted face ids do not indicate this order.
    #[serde(default)]
    pub d2: Option<Num>,
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name. Names interpreted as numeric ids are refused.
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
    /// Hole centre as a shift [u, v] from the face's area centroid (as the face stands: other cut-outs in it move
    /// the centroid) along the face's axes, mm, numbers or expressions; it
    /// follows parameters (QymCAD's off_u/off_v). Axes: on a face facing ±Z, u = +X and v = +Y on a top face (−Y
    /// on a bottom face); otherwise u is horizontal (Z × normal) and v = normal × u. Excludes `at`.
    #[serde(default)]
    pub offset: Option<[Num; 2]>,
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
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name. Names interpreted as numeric ids are refused.
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
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name. Names interpreted as numeric ids are refused.
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
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name. Names interpreted as numeric ids are refused.
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
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name. Names interpreted as numeric ids are refused.
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
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name. Names interpreted as numeric ids are refused.
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
    /// Name for the new feature (timeline node), usable instead of its id later. Default: QymCAD's generic name. Names interpreted as numeric ids are refused.
    #[serde(default)]
    pub name: Option<String>,
}

fn opt_body(s: &Session, b: &Option<ObjRef>) -> Result<Option<Id>, String> {
    b.as_ref().map(|b| b.resolve(s)).transpose()
}

/// Contour ids given directly, or picked by points (`profiles_at`); not both.
fn picked_profiles(s: &Session, sketch: Id, ids: Option<Vec<Id>>, at: Option<Vec<[Num; 2]>>) -> Result<Option<Vec<Id>>, String> {
    match (ids, at) {
        (Some(_), Some(_)) => Err("give `profiles` or `profiles_at`, not both".into()),
        (None, Some(points)) => s.contours_at(sketch, &points).map(Some).map_err(err),
        (ids, None) => Ok(ids),
    }
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
             {\"base\": \"XY\", \"dist\": \"t\"}). Returns its id; sketch on it with {\"plane\": id}. For an exact pocket floor, sketch on the top face or a plane at the top and cut in reverse; the 0.001 mm entry clearance then lies outside the material.",
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
                 For an exact pocket floor, sketch on the top face or a plane at the top and cut in reverse; the 0.001 mm entry clearance then lies outside the material. \
                 Optional down gives native Two sides with independent positive lengths: normal spans [-down, height], reverse [-height, down]. Both lengths stay parametric; down must remain finite and greater than 1e-9, including later edits, and cannot be combined with symmetric or through. Recreate the feature to return to one side. Two-sided cuts use their exact ends without one-sided entry clearance. \
                 Cuts ending at the far stock boundary also receive clearance to become through cuts; through=true goes to the end of the stock on the chosen side; with direction symmetric it spans both sides of the sketch plane.",
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
                let profiles = picked_profiles(s, sketch, a.profiles, a.profiles_at)?;
                let x = Extrude {
                    down: a.down,
                    sketch,
                    profiles,
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
             part's body), cut, intersect, or a new body. Atomic. Pinned OCCT can refuse a later cone-mouth chamfer because of surface parameterization, including overlapping cones; orientation alone does not predict failure. If the distance fits, for a full turn draw the axis line toward the sketch's +y with the profile at larger x than the line, or reverse the construction-axis line endpoints. For a partial turn, reversing endpoints changes the sweep.",
                stale!()
            ),
            |st, a: RevolveArgs| {
                let s = st.doc()?;
                let sketch = a.sketch.resolve(s)?;
                let x = Revolve {
                    sketch,
                    profiles: picked_profiles(s, sketch, a.profiles, a.profiles_at)?,
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
            "sweep",
            concat!(
                "Sweep closed profile contours of a sketch along a path: a contour of another sketch, or a chain of \
                 body edges (kept by their persistent names, so the path follows the body). The profile stays where it \
                 is drawn when the path starts on its plane; otherwise it is carried to the path's start. add (the \
                 first add creates the part's body), cut, intersect or new_body. A sweep has no size of its own: the \
                 profile and path sketches carry the formulas. Atomic.",
                stale!()
            ),
            |st, a: SweepArgs| {
                let s = st.doc()?;
                let sketch = a.sketch.resolve(s)?;
                let path = match a.path {
                    PathArg::Sketch(p) => SweepPath::Sketch { sketch: p.sketch.resolve(s)?, profile: p.profile },
                    PathArg::Edges(p) => SweepPath::Edges { body: opt_body(s, &p.body)?, edges: p.edges.resolve(s)? },
                };
                let x = Sweep {
                    sketch,
                    profiles: picked_profiles(s, sketch, a.profiles, a.profiles_at)?,
                    path,
                    op: a.op,
                    target: opt_body(s, &a.target)?,
                    name: a.name,
                };
                feature_json(s.sweep(&x))
            },
        ),
        tool(
            "loft",
            concat!(
                "Loft a solid (or, with surface and op new_body, a sheet) through closed section contours in order, \
                 usually sketches on parallel datum planes from plane_offset. ruled gives straight faces; the default \
                 is smooth. add (the first add creates the part's body), cut, intersect or new_body. A loft has no size \
                 of its own: the sections and the planes they stand on carry the formulas. Atomic.",
                stale!()
            ),
            |st, a: LoftArgs| {
                let s = st.doc()?;
                let mut sections = Vec::new();
                for section in a.sections {
                    let sketch = section.sketch.resolve(s)?;
                    let profile = match (section.profile, section.profile_at) {
                        (Some(_), Some(_)) => return Err("give `profile` or `profile_at`, not both".into()),
                        (None, Some(at)) => Some(s.contours_at(sketch, &[at]).map_err(err)?[0]),
                        (profile, None) => profile,
                    };
                    sections.push(LoftSection { sketch, profile });
                }
                let x = Loft { sections, ruled: a.ruled, op: a.op, target: opt_body(s, &a.target)?, surface: a.surface, name: a.name };
                feature_json(s.loft(&x))
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
             for the geometry is an error and nothing changes. Seam edges are dropped automatically and counted in rebuild.notes; a selection containing only seams is refused. During blending, the kernel extends a blend along edges tangent-continuous with a selected edge; preview with select shows only the selected edges, including seams.",
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
             fillet. QymCAD places `dist` on its first adjacent face in native traversal order, which is not reported. Inspect adjacent face setbacks before/after; on straight rectangular faces, lost area / edge length gives the setback. Topology's sorted face ids do not indicate this order. Seam edges are dropped automatically and counted in rebuild.notes; a selection containing only seams is refused. During blending, the kernel extends a blend along edges tangent-continuous with a selected edge; preview with select shows only the selected edges, including seams.",
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
             such a description is stored and keeps working after upstream edits. Kind-filter leaves are frozen to persistent face ids at creation and do not rediscover later faces. `at` is a world point. \
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
                    offset: a.offset,
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
                "Hollow a body with walls of `thickness`, removing `open_faces` (at least one; e.g. {\"facing\": \"+z\"} for an open box). Kind-filter leaves are frozen to persistent face ids at creation and do not rediscover later faces.",
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
            concat!("Move one planar face along its outward normal by `dist` (negative = into the body), e.g. raise the top. Kind-filter leaves are frozen to persistent face ids at creation and do not rediscover later faces.", stale!()),
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
