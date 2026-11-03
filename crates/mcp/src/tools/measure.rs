//! Read-only native support-geometry measurements with strict operand decoding.
use super::{
    common::{err, ObjRef},
    tool,
    topology::SelArg,
    Tool,
};
use qymcad_engine::{MeasureAt, MeasureOperand, Session};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
enum Operand {
    Point {
        /// Finite world coordinates [x, y, z], in mm.
        point: [f64; 3],
    },
    Face {
        /// Body id or name containing the current face.
        body: ObjRef,
        /// Existing selection syntax (ids or descriptions); must resolve to exactly one current face.
        face: SelArg,
    },
    Edge {
        /// Body id or name containing the current edge.
        body: ObjRef,
        /// Existing selection syntax (ids or descriptions); must resolve to exactly one current edge.
        edge: SelArg,
        /// Convert the edge to its current start, end, midpoint or circle/arc centre. Center requires a circle/arc.
        #[serde(default)]
        at: Option<MeasureAt>,
    },
}

impl Operand {
    fn resolve(self, session: &Session) -> Result<MeasureOperand, String> {
        Ok(match self {
            Self::Point { point } => MeasureOperand::Point(point),
            Self::Face { body, face } => MeasureOperand::Face { body: body.resolve(session)?, selection: face.resolve(session)? },
            Self::Edge { body, edge, at } => MeasureOperand::Edge { body: body.resolve(session)?, selection: edge.resolve(session)?, at },
        })
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MeasureArgs {
    /// First operand: exactly {point:[x,y,z]}, {body,face:selection}, or {body,edge:selection,at?:start|end|mid|center}.
    a: Operand,
    /// Optional second operand. Omit for a line/spline length or circle/arc/cylinder radius and diameter; points and planar faces need a second operand.
    #[serde(default)]
    b: Option<Operand>,
}

pub fn tools() -> Vec<Tool> {
    vec![tool("measure", "Read-only native QymCAD measurement of one or two operands. Uses infinite support lines/planes/cylinders and full underlying circles even for arcs; this is not general trimmed B-rep minimum distance. Single edges report length (real arc span; splines labelled polyline_approximation); circles/arcs and cylinder faces report radius/diameter. Pairs support points, straight lines, circles/arcs, planes and cylinders; spline edges, cone/sphere/other faces and native fallback anchor pairs are refused. Circle pairs report centre distance; parallel cylinders report signed_axis_clearance (axis gap minus radii, possibly negative), never wall distance. Angles are acute unsigned. Use edge at:start|end|mid|center to measure from a point. Results use full-precision world mm and degrees with resolved current ids; the app displays active-context coordinates, so signed deltas may differ for a placed part while distance/angle do not. No area, mass or signed angles. Read tools may restore the derived face cache after open (including on refusal), adding faces/*.ron entries to the next save; recipe, B-reps and diagnostics do not change. No undo entry or batch support.", |st, args: MeasureArgs| {
        let session = st.doc()?;
        let a = args.a.resolve(session)?;
        let b = args.b.map(|operand| operand.resolve(session)).transpose()?;
        Ok(json!(session.measure(a, b).map_err(err)?))
    })]
}
