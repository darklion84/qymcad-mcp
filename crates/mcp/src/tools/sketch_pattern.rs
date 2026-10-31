use super::common::{err, rebuild_json, ObjRef};
use super::{tool, Tool};
use qymcad_engine::{Num, SketchPatternKind};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PatternArgs {
    /// Sketch id or name.
    pub sketch: ObjRef,
    /// Non-empty source curve ids on create. On update, omit or repeat the stored source exactly.
    #[serde(default)]
    pub entities: Option<Vec<u64>>,
    /// Existing pattern id or name for update; omit for creation.
    #[serde(default)]
    pub pattern: Option<ObjRef>,
    /// Linear or circular sketch pattern.
    pub kind: PatternKindArg,
    /// Number of copies, integral from 2 to 1000. Total grid copies are at most 1000.
    pub count: Num,
    /// Linear step along x, required for linear patterns.
    #[serde(default)]
    pub dx: Option<Num>,
    /// Linear step along y, required for linear patterns.
    #[serde(default)]
    pub dy: Option<Num>,
    /// Optional second linear-direction count; omitted means one row.
    #[serde(default)]
    pub count2: Option<Num>,
    /// Optional second linear step along x.
    #[serde(default)]
    pub dx2: Option<Num>,
    /// Optional second linear step along y.
    #[serde(default)]
    pub dy2: Option<Num>,
    /// Circular centre [x,y], evaluated once and not a native formula.
    #[serde(default)]
    pub centre: Option<[Num; 2]>,
    /// Circular total angle in degrees. Full circles use total/count spacing; sectors use total/max(count,2).
    #[serde(default)]
    pub angle: Option<Num>,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PatternKindArg {
    Linear,
    Circular,
}

pub(super) fn tools() -> Vec<Tool> {
    vec![tool("sketch_pattern", "Create or update a native linked sketch pattern. Linear patterns use count, dx, dy and optional count2/dx2/dy2; circular patterns use count, centre and total angle. Native circular spacing is total/count for a full circle and total/max(count,2) for a sector. Counts are strict integral values 2..=1000, with total copies capped at 1000; invalid edits roll back before native work. Instances are derived results and must be edited through the pattern.", |st, a: PatternArgs| {
        let s = st.doc()?;
        let sketch = a.sketch.resolve(s)?;
        let kind = match a.kind {
            PatternKindArg::Linear => SketchPatternKind::Linear {
                count: a.count,
                dx: a.dx.ok_or_else(|| "linear pattern requires dx".to_string())?,
                dy: a.dy.ok_or_else(|| "linear pattern requires dy".to_string())?,
                count2: a.count2,
                dx2: a.dx2,
                dy2: a.dy2,
            },
            PatternKindArg::Circular => SketchPatternKind::Circular {
                count: a.count,
                centre: a.centre.ok_or_else(|| "circular pattern requires centre".to_string())?,
                angle: a.angle.ok_or_else(|| "circular pattern requires angle".to_string())?,
            },
        };
        let r = match a.pattern {
            Some(pattern) => s.sketch_pattern_update(sketch, pattern.resolve(s)?, a.entities.as_deref(), &kind).map_err(err)?,
            None => s.sketch_pattern_create(sketch, a.entities.as_deref().ok_or("create requires entities")?, &kind).map_err(err)?,
        };
        Ok(json!({"pattern":r.pattern,"source":r.source,"created_instances":r.created_instances,"removed_instances":r.removed_instances,"evaluated":r.evaluated,"formulas":r.formulas,"names":r.names,"contours":r.contours,"rebuild":r.rebuild.as_ref().map(rebuild_json)}))
    })]
}
