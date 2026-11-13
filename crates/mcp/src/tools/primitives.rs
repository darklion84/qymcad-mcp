//! Primitives: the app's `part.box|cylinder|sphere|cone|torus|prism` commands as one tool with a `kind`.

use super::common::{err, rebuild_json, ObjRef};
use super::datums::{VertexArgs, VertexEndArg};
use super::{tool, Tool};
use qymcad_engine::{Num, Placement, PrimitiveSpec, VertexEnd};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

/// Where the primitive stands. Give `point` or `vertex`, or omit the whole `place` for the Part's origin.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceArgs {
    /// A datum point of the active Part (id or name): the primitive's reference point goes to it. A pure translation, as the app's click: the primitive does NOT follow the point afterwards.
    #[serde(default)]
    pub point: Option<ObjRef>,
    /// An end of a persistent edge of a current body of the active Part (ids from `topology`): a pure translation to that vertex, no association.
    #[serde(default)]
    pub vertex: Option<VertexArgs>,
}

macro_rules! primitive_args {
    ($(#[$meta:meta])* $name:ident { $($(#[$fmeta:meta])* $field:ident),* }) => {
        $(#[$meta])*
        #[derive(Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            $($(#[$fmeta])* pub $field: Num,)*
            /// Where it stands: `{point}` or `{vertex}`. Default: the Part's origin (as the app without a click). Planes and faces are not covered (use move or part_place afterwards).
            #[serde(default)]
            pub place: Option<PlaceArgs>,
            /// Name for the primitive's node (what `dimension_set` targets); the resulting body keeps the id in `body`.
            #[serde(default)]
            pub name: Option<String>,
        }
    };
}

primitive_args!(
    /// Box, standing on z = 0 and centred on XY.
    BoxArgs {
        /// Length along X, mm: 0.1..=100000 (number or formula).
        dx,
        /// Length along Y, mm: 0.1..=100000.
        dy,
        /// Height along Z, mm: 0.1..=100000.
        dz
    }
);
primitive_args!(
    /// Cylinder along Z, base on z = 0, centred on XY.
    CylinderArgs {
        /// Radius, mm: 0.05..=100000.
        r,
        /// Height, mm: 0.1..=100000.
        h
    }
);
primitive_args!(
    /// Sphere centred on the origin.
    SphereArgs {
        /// Radius, mm: 0.05..=100000.
        r
    }
);
primitive_args!(
    /// Cone or truncated cone along Z, base (r1) on z = 0.
    ConeArgs {
        /// Base radius, mm: 0.01..=100000 at creation (an edit may bring it to 0, an apex at the bottom). Must differ from r2.
        r1,
        /// Top radius, mm: 0..=100000 (0 is a sharp cone). Must differ from r1: equal radii make the app's cone fail (F-182); use a cylinder.
        r2,
        /// Height, mm: 0.1..=100000.
        h
    }
);
primitive_args!(
    /// Torus about Z, centred on the origin, tube centre circle in z = 0.
    TorusArgs {
        /// Radius of the ring (centre of the tube), mm: 0.1..=100000.
        major,
        /// Radius of the tube, mm: 0.1 up to max(major - 0.001, 0.1) (the app's field).
        minor
    }
);
primitive_args!(
    /// Regular prism along Z, base on z = 0, circumscribed radius r, first vertex on +X.
    PrismArgs {
        /// Circumscribed radius, mm: 0.1..=100000.
        r,
        /// Height, mm: 0.1..=100000.
        h,
        /// Number of sides: a whole number 3..=64 (number or a formula that gives one, also after later edits).
        n
    }
);

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PrimitiveArgs {
    Box(BoxArgs),
    Cylinder(CylinderArgs),
    Sphere(SphereArgs),
    Cone(ConeArgs),
    Torus(TorusArgs),
    Prism(PrismArgs),
}

pub fn tools() -> Vec<Tool> {
    vec![tool(
        "primitive",
        "Create a primitive, as the app's Box, Cylinder, Sphere, Cone, Torus and Prism commands do: `kind` box {dx, dy, dz} | cylinder {r, h} | sphere {r} | cone {r1, r2, h} | torus {major, minor} | prism {r, h, n}, every size a number or a formula (formulas stay on the node and follow parameters). The app's field limits hold at creation and on every later edit, because the core builds some sizes below them silently wrong (a thin extrusion under 1e-5 mm has a third of its volume, F-180): box edges, cylinder h, cone h, torus and prism lengths 0.1..=100000 mm, cylinder/sphere r 0.05..=100000, cone r1 0.01..=100000 (0 allowed on edit) and r2 0..=100000 with r1 different from r2 (F-182), torus minor at most max(major - 0.001, 0.1), prism n a WHOLE number 3..=64 (the app rounds and clamps it silently, F-181); the fields' own 1e-9 float tolerance applies. A file made in the app that is already outside a field opens with a warning and keeps that size tolerated while it is unchanged; its other sizes, and that size moved, are judged as above. Standing: box, cylinder, cone and prism sit on z = 0 centred on XY, sphere and torus are centred on the origin. `place` {point: datum point} or {vertex: {edge, end, body?}} translates it there as the app's click does (pure translation, no association: the point or vertex does not lead it afterwards); the point must be built (not beyond the rollback bar), the vertex on an edge the app's click reaches (not a closed or smooth edge); planes and faces are not covered (use move or part_place). A Part is one body: like the app, the primitive is UNITED with the Part's current body (the last one when several exist, F-184); the first primitive in an empty Part is the body. A primitive over a body that is not built (suppressed, or in a suppressed chain) is refused: the app would give a silently empty result (F-185). A far-away or touching primitive makes one body of two pieces (correct volumes). Suppressing a united primitive unbuilds the union and so hides the Part's body (F-183); feature_delete needs cascade for the union. Not offered while an assembly (root or nested) is the active context (part_select a Part first). Returns feature (the primitive's node: target of dimension_set, keys are the size names dx, dy, dz, r, h, r1, r2, major, minor, n), body (the Part's body after the action, one of three: the primitive's node itself when it stands unplaced in an empty Part; its placement Move node when placed into an empty Part; the union node when the Part already had a body), kind, name, sizes (evaluated after the rebuild), placed_at, joined {prior, union} and rebuild. One undo step, batch-allowed.",
        |st, a: PrimitiveArgs| {
            let s = st.doc()?;
            let (spec, place, name) = match a {
                PrimitiveArgs::Box(b) => (PrimitiveSpec::Box { dx: b.dx, dy: b.dy, dz: b.dz }, b.place, b.name),
                PrimitiveArgs::Cylinder(b) => (PrimitiveSpec::Cylinder { r: b.r, h: b.h }, b.place, b.name),
                PrimitiveArgs::Sphere(b) => (PrimitiveSpec::Sphere { r: b.r }, b.place, b.name),
                PrimitiveArgs::Cone(b) => (PrimitiveSpec::Cone { r1: b.r1, r2: b.r2, h: b.h }, b.place, b.name),
                PrimitiveArgs::Torus(b) => (PrimitiveSpec::Torus { major: b.major, minor: b.minor }, b.place, b.name),
                PrimitiveArgs::Prism(b) => (PrimitiveSpec::Prism { r: b.r, h: b.h, n: b.n }, b.place, b.name),
            };
            let placement = match place {
                None => Placement::Origin,
                Some(PlaceArgs { point: Some(p), vertex: None }) => Placement::Point(p.resolve(s)?),
                Some(PlaceArgs { point: None, vertex: Some(v) }) => Placement::Vertex {
                    edge: v.edge,
                    end: match v.end {
                        VertexEndArg::Start => VertexEnd::Start,
                        VertexEndArg::End => VertexEnd::End,
                    },
                    body: v.body.as_ref().map(|b| b.resolve(s)).transpose()?,
                },
                Some(PlaceArgs { point: None, vertex: None }) => return Err("`place` is empty: give `point` or `vertex`, or omit `place` for the origin".into()),
                Some(_) => return Err("give exactly one of `point` or `vertex` in `place`".into()),
            };
            let (info, r) = s.primitive(&spec, &placement, name.as_deref()).map_err(err)?;
            let mut out = json!(info);
            out["rebuild"] = rebuild_json(&r);
            Ok(out)
        },
    )]
}
