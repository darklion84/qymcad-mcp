//! Topology tools (`topology`, `select`) and the JSON forms shared with the feature tools: selections of faces
//! and edges, directions and axes. These are parsed by hand from JSON so a malformed selection gets a precise
//! error instead of serde's "did not match any variant".

use super::common::{err, round, ObjRef};
use super::{tool, Tool};
use qymcad_engine::{Axis, AxisRef, EdgeKind, Element, FaceKind, Id, Role, Sel, SelectionKind, Session, Topology};
use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer};
use serde_json::{json, Map, Value};
use std::borrow::Cow;

const SEL_HELP: &str = "A selection of faces or edges. Explicit ids from `topology`: [id, ...] or a single id \
    (valid only for the body they were read from, after the latest feature). Or a description, re-evaluated by \
    QymCAD: {\"facing\": \"+z\"} faces whose outward normal points that way (\"+x\" ... \"-z\" or [x,y,z]; \
    optional \"tol_deg\", default 5); {\"along\": \"z\"} edges running along a direction (either sense); \
    {\"concave\": true} / {\"convex\": true} inward/outward corners between two distinct faces, including curved \
    junctions (e.g. boss/plate and hole rims); seams and G1 tangent junctions are excluded; \
    uncertain shallow signs or disagreement among five arc-length samples are omitted. Analytic cone rims \
    at slopes >=3 degrees are tested; 0.9 degrees may be omitted (native smoothness threshold about 1.5 degrees), \
    and fallback facet normals can require \
    about 20–30 degrees. Bare corner and positive `and` previews count omitted uncertain candidates; \
    an `and` requiring both signs matches no edge and reports no omission. Other compositions report the \
    uncertain body total: corner filters treat those edges as neither concave nor convex; \
    {\"extreme\": \"+z\"} the topmost faces/edges (\"-x\" = leftmost ...); \"largest\" the largest face / longest \
    edge; {\"of_feature\": <feature id or name>, \"role\": \"cap_end\"} faces made by a feature (roles: cap_start, \
    cap_end = far cap of an extrude, wall, revolved, hole, blend, shell_wall); {\"edges_of\": <face selection>} \
    the edges bounding faces; {\"tangent_chain\": <edge selection>} edges continuing them smoothly; \
    {\"between\": [<faces>, <faces>]} edges where the two face sets meet; {\"union\": [...]}, \
    {\"minus\": [a, b]}, {\"and\": [a, b, ...]} intersection of at least two selections. \
    {\"kind\": \"line\"|\"circle\"|\"arc\"|\"curve\"|\"other\"} edges; curve combines arc/other and excludes \
    full circles. {\"kind\": \"plane\"|\"cylinder\"|\"cone\"|\"sphere\"|\"other\"} faces. Kinds use topology's \
    classification. In hole/shell/push_face, any selection containing a kind freezes its whole resolved face set \
    as persistent picks at creation; it does not rediscover faces after edits. Selections without kinds stay dynamic. Example, the top outline of a block: \
    {\"edges_of\": {\"facing\": \"+z\"}}.";

/// A selection as the agent writes it (names still unresolved).
#[derive(Clone, Debug)]
pub enum SelArg {
    Ids(Vec<u32>),
    /// Geometric face/edge kind, using the topology taxonomy.
    Kind(SelectionKind),
    OfFeature {
        /// Feature id or name whose generated faces to select.
        feature: ObjRef,
        /// Restrict to this face role; omitted selects every face of the feature.
        role: Option<Role>,
    },
    Facing {
        /// World direction to compare with each planar face's outward normal.
        dir: [f64; 3],
        /// Angular tolerance in degrees, in [0, 90); default 5.
        tol_deg: f64,
    },
    Along {
        /// World direction of straight edges, in either sense.
        dir: [f64; 3],
        /// Angular tolerance in degrees, in [0, 90); default 5.
        tol_deg: f64,
    },
    Concave,
    Convex,
    Extreme {
        /// World axis along which to find the extreme faces or edges.
        axis: Axis,
        /// True selects the positive extreme; false selects the negative extreme.
        max: bool,
    },
    Largest,
    EdgesOf(Box<SelArg>),
    TangentChain {
        /// Edges from which to follow a chain of smoothly continuing edges.
        seed: Box<SelArg>,
        /// Angular tolerance in degrees, in [0, 90); default 5.
        tol_deg: f64,
    },
    Between(Box<SelArg>, Box<SelArg>),
    Union(Vec<SelArg>),
    Minus(Box<SelArg>, Box<SelArg>),
    And(Box<SelArg>, Box<SelArg>),
}

impl SelArg {
    pub fn resolve(&self, s: &Session) -> Result<Sel, String> {
        let b = |x: &SelArg| x.resolve(s).map(Box::new);
        Ok(match self {
            SelArg::Ids(v) => Sel::Ids(v.clone()),
            SelArg::Kind(kind) => Sel::Kind(*kind),
            SelArg::OfFeature { feature, role } => Sel::OfFeature { feature: feature.resolve(s)?, role: *role },
            SelArg::Facing { dir, tol_deg } => Sel::Facing { dir: *dir, tol_deg: *tol_deg },
            SelArg::Along { dir, tol_deg } => Sel::Along { dir: *dir, tol_deg: *tol_deg },
            SelArg::Concave => Sel::Concave,
            SelArg::Convex => Sel::Convex,
            SelArg::Extreme { axis, max } => Sel::Extreme { axis: *axis, max: *max },
            SelArg::Largest => Sel::Largest,
            SelArg::EdgesOf(x) => Sel::EdgesOf(b(x)?),
            SelArg::TangentChain { seed, tol_deg } => Sel::TangentChain { seed: b(seed)?, tol_deg: *tol_deg },
            SelArg::Between(x, y) => Sel::Between(b(x)?, b(y)?),
            SelArg::Union(v) => Sel::Union(v.iter().map(|x| x.resolve(s)).collect::<Result<_, _>>()?),
            SelArg::Minus(x, y) => Sel::Minus(b(x)?, b(y)?),
            SelArg::And(x, y) => Sel::And(b(x)?, b(y)?),
        })
    }

    fn parse(v: &Value) -> Result<SelArg, String> {
        match v {
            Value::Number(_) => Ok(SelArg::Ids(vec![id_u32(v)?])),
            Value::Array(a) => Ok(SelArg::Ids(a.iter().map(id_u32).collect::<Result<_, _>>()?)),
            Value::String(s) if s == "largest" => Ok(SelArg::Largest),
            Value::Object(o) => Self::parse_object(o),
            other => Err(format!("not a selection: {other} (expected ids, \"largest\" or an object such as {{\"facing\": \"+z\"}})")),
        }
    }

    fn parse_object(o: &Map<String, Value>) -> Result<SelArg, String> {
        const KEYS: [&str; 14] = [
            "ids",
            "kind",
            "of_feature",
            "facing",
            "along",
            "concave",
            "convex",
            "extreme",
            "edges_of",
            "tangent_chain",
            "between",
            "union",
            "minus",
            "and",
        ];
        let found: Vec<&str> = KEYS.iter().copied().filter(|k| o.contains_key(*k)).collect();
        let key = match found.as_slice() {
            [k] => *k,
            [] => return Err(format!("unknown selection {}: use one of {KEYS:?}", Value::Object(o.clone()))),
            many => return Err(format!("a selection object takes one of {many:?}, not several; combine with `union`/`and`")),
        };
        let allowed: &[&str] = match key {
            "of_feature" => &["of_feature", "role"],
            "facing" | "along" | "tangent_chain" => &[key, "tol_deg"],
            _ => &[key],
        };
        if let Some(extra) = o.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(format!("unexpected `{extra}` in a `{key}` selection (allowed: {allowed:?})"));
        }
        let val = &o[key];
        let tol = |default: f64| -> Result<f64, String> {
            match o.get("tol_deg") {
                None => Ok(default),
                Some(t) => tol_deg(t.as_f64().ok_or_else(|| format!("tol_deg must be a number, got {t}"))?),
            }
        };
        let pair = |what: &str| -> Result<(Box<SelArg>, Box<SelArg>), String> {
            match val.as_array().map(Vec::as_slice) {
                Some([a, b]) => Ok((Box::new(Self::parse(a)?), Box::new(Self::parse(b)?))),
                _ => Err(format!("`{what}` takes a list of two selections")),
            }
        };
        Ok(match key {
            "ids" => match val {
                Value::Array(_) => Self::parse(val)?,
                _ => return Err("`ids` takes a list of ids".into()),
            },
            "kind" => SelArg::Kind(
                serde_json::from_value(val.clone())
                    .map_err(|_| format!("unknown kind {val}: line, circle, arc, curve, plane, cylinder, cone, sphere, other"))?,
            ),
            "of_feature" => {
                let feature = serde_json::from_value::<ObjRef>(val.clone())
                    .map_err(|_| format!("of_feature: expected an id or a name, got {val}"))?;
                let role = match o.get("role") {
                    None => None,
                    Some(r) => Some(
                        serde_json::from_value::<Role>(r.clone())
                            .map_err(|_| format!("unknown role {r}: cap_start, cap_end, wall, revolved, hole, blend, shell_wall"))?,
                    ),
                };
                SelArg::OfFeature { feature, role }
            }
            "facing" => SelArg::Facing { dir: dir(val)?, tol_deg: tol(5.0)? },
            "along" => SelArg::Along { dir: dir(val)?, tol_deg: tol(5.0)? },
            "concave" | "convex" => {
                if val != &Value::Bool(true) {
                    return Err(format!("`{key}` takes true"));
                }
                if key == "concave" {
                    SelArg::Concave
                } else {
                    SelArg::Convex
                }
            }
            "extreme" => {
                let (axis, max) = signed_axis(val)?;
                SelArg::Extreme { axis, max }
            }
            "edges_of" => SelArg::EdgesOf(Box::new(Self::parse(val)?)),
            "tangent_chain" => SelArg::TangentChain { seed: Box::new(Self::parse(val)?), tol_deg: tol(5.0)? },
            "between" => {
                let (a, b) = pair("between")?;
                SelArg::Between(a, b)
            }
            "minus" => {
                let (a, b) = pair("minus")?;
                SelArg::Minus(a, b)
            }
            "and" => {
                let items =
                    val.as_array().filter(|v| v.len() >= 2).ok_or_else(|| "`and` takes a list of at least two selections".to_string())?;
                balanced_and(items.iter().map(Self::parse).collect::<Result<_, _>>()?)?
            }
            _ => match val.as_array() {
                Some(v) if !v.is_empty() => SelArg::Union(v.iter().map(Self::parse).collect::<Result<_, _>>()?),
                _ => return Err("`union` takes a non-empty list of selections".into()),
            },
        })
    }
}

/// Keep a wide intersection within the same query-depth budget as a wide union (F-032).
fn balanced_and(mut items: Vec<SelArg>) -> Result<SelArg, String> {
    match items.len() {
        0 => Err("`and` takes a list of at least two selections".into()),
        1 => items.pop().ok_or_else(|| "`and` takes a list of at least two selections".into()),
        n => {
            let right = items.split_off(n / 2);
            Ok(SelArg::And(Box::new(balanced_and(items)?), Box::new(balanced_and(right)?)))
        }
    }
}

impl<'de> Deserialize<'de> for SelArg {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        SelArg::parse(&v).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for SelArg {
    fn schema_name() -> Cow<'static, str> {
        "Selection".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": SEL_HELP,
            "anyOf": [
                { "type": "array", "items": { "type": "integer", "minimum": 0 } },
                { "type": "integer", "minimum": 0 },
                { "type": "string", "enum": ["largest"] },
                { "type": "object" }
            ]
        })
    }
}

fn id_u32(v: &Value) -> Result<u32, String> {
    v.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| format!("not a face/edge id: {v}"))
}

/// Exactly three finite numbers, `[x, y, z]`. Anything else is an error (never dropped or padded).
fn vec3(v: &Value) -> Result<[f64; 3], String> {
    let bad = || format!("expected [x, y, z] (three finite numbers), got {v}");
    let a = v.as_array().filter(|a| a.len() == 3).ok_or_else(bad)?;
    let mut out = [0.0; 3];
    for (o, x) in out.iter_mut().zip(a) {
        *o = x.as_f64().filter(|f| f.is_finite()).ok_or_else(bad)?;
    }
    Ok(out)
}

/// A tolerance in degrees: finite, in [0, 90).
fn tol_deg(t: f64) -> Result<f64, String> {
    if t.is_finite() && (0.0..90.0).contains(&t) {
        Ok(t)
    } else {
        Err(format!("tol_deg must be a number of degrees in [0, 90), got {t}"))
    }
}

/// "+x" / "-y" / "z" (= "+z", case-insensitive) or [x, y, z].
fn dir(v: &Value) -> Result<[f64; 3], String> {
    if v.is_array() {
        return vec3(v);
    }
    let (axis, max) = signed_axis(v).map_err(|error| format!("{error}; alternatively give [x, y, z] (three finite numbers)"))?;
    let s = if max { 1.0 } else { -1.0 };
    Ok(match axis {
        Axis::X => [s, 0.0, 0.0],
        Axis::Y => [0.0, s, 0.0],
        Axis::Z => [0.0, 0.0, s],
    })
}

fn signed_axis(v: &Value) -> Result<(Axis, bool), String> {
    let bad = || format!("expected \"x\", \"y\", \"z\", \"+x\", \"-x\", \"+y\", \"-y\", \"+z\" or \"-z\" (case-insensitive), got {v}");
    let s = v.as_str().ok_or_else(bad)?.trim().to_lowercase();
    let (neg, name) = match s.strip_prefix('-') {
        Some(rest) => (true, rest.to_string()),
        None => (false, s.strip_prefix('+').unwrap_or(&s).to_string()),
    };
    let axis = match name.as_str() {
        "x" => Axis::X,
        "y" => Axis::Y,
        "z" => Axis::Z,
        _ => return Err(bad()),
    };
    Ok((axis, !neg))
}

const AXIS_HELP: &str = "An axis. For revolve: \"sketch_x\" or \"sketch_y\" (the sketch's own axes), or {\"line\": <line id>} \
    (a line of that sketch, e.g. from a construction rectangle). Anywhere: \"X\", \"Y\", \"Z\" (world axes through \
    the origin), {\"origin\": [x,y,z], \"dir\": [x,y,z]} (a fixed axis), {\"face\": <face id>, \"body\": <body>} (the \
    axis of a cylindrical or conical face; follows the face; body defaults to the current body), {\"datum\": <datum \
    axis id>}. A revolve axis must lie in the sketch plane and must not cross the profile.";

/// An axis as the agent writes it.
#[derive(Clone, Debug)]
pub enum AxisArg {
    SketchX,
    SketchY,
    World(Axis),
    Line(Id),
    Datum(Id),
    Through {
        /// Point on the fixed world axis [x, y, z], mm.
        origin: [f64; 3],
        /// Nonzero direction of the fixed world axis [x, y, z].
        dir: [f64; 3],
    },
    Face {
        /// Cylindrical or conical face id from the body's current topology.
        face: u32,
        /// Body id or name containing the face. Default: the part's current body.
        body: Option<ObjRef>,
    },
}

impl AxisArg {
    pub fn resolve(&self, s: &Session) -> Result<AxisRef, String> {
        Ok(match self {
            AxisArg::SketchX => AxisRef::SketchX,
            AxisArg::SketchY => AxisRef::SketchY,
            AxisArg::World(a) => AxisRef::World(*a),
            AxisArg::Line(l) => AxisRef::Line(*l),
            AxisArg::Datum(d) => AxisRef::Datum(*d),
            AxisArg::Through { origin, dir } => AxisRef::Through { origin: *origin, dir: *dir },
            // The default body is resolved by the engine (the current body of the active part).
            AxisArg::Face { face, body } => AxisRef::FaceAxis { body: body.as_ref().map(|b| b.resolve(s)).transpose()?, face: *face },
        })
    }

    fn parse(v: &Value) -> Result<AxisArg, String> {
        match v {
            Value::String(s) => match s.as_str() {
                "sketch_x" => Ok(AxisArg::SketchX),
                "sketch_y" => Ok(AxisArg::SketchY),
                "X" | "x" => Ok(AxisArg::World(Axis::X)),
                "Y" | "y" => Ok(AxisArg::World(Axis::Y)),
                "Z" | "z" => Ok(AxisArg::World(Axis::Z)),
                _ => Err(format!("unknown axis \"{s}\": sketch_x, sketch_y, X, Y, Z or an object")),
            },
            Value::Object(o) => {
                let keys: Vec<&str> = o.keys().map(String::as_str).collect();
                let id = |k: &str| o[k].as_u64().ok_or_else(|| format!("`{k}` takes an id, got {}", o[k]));
                match keys.as_slice() {
                    ["line"] => Ok(AxisArg::Line(id("line")?)),
                    ["datum"] => Ok(AxisArg::Datum(id("datum")?)),
                    _ if o.contains_key("origin") && o.contains_key("dir") && o.len() == 2 => {
                        Ok(AxisArg::Through { origin: vec3(&o["origin"])?, dir: vec3(&o["dir"])? })
                    }
                    _ if o.contains_key("face") && o.keys().all(|k| k == "face" || k == "body") => {
                        let face = id_u32(&o["face"])?;
                        let body = match o.get("body") {
                            Some(b) => Some(
                                serde_json::from_value::<ObjRef>(b.clone())
                                    .map_err(|_| format!("body: expected an id or a name, got {b}"))?,
                            ),
                            None => None,
                        };
                        Ok(AxisArg::Face { face, body })
                    }
                    _ => Err(format!("unknown axis {v}: {{\"line\"}}, {{\"datum\"}}, {{\"origin\",\"dir\"}} or {{\"face\",\"body\"}}")),
                }
            }
            _ => Err(format!("unknown axis {v}")),
        }
    }
}

impl<'de> Deserialize<'de> for AxisArg {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        AxisArg::parse(&v).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for AxisArg {
    fn schema_name() -> Cow<'static, str> {
        "Axis".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": AXIS_HELP,
            "anyOf": [
                { "type": "string", "enum": ["sketch_x", "sketch_y", "X", "Y", "Z"] },
                { "type": "object" }
            ]
        })
    }
}

/// A direction as the agent writes it: "+x" ... "-z" or [x, y, z].
#[derive(Clone, Copy, Debug)]
pub struct DirArg(pub [f64; 3]);

impl<'de> Deserialize<'de> for DirArg {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        dir(&v).map(DirArg).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for DirArg {
    fn schema_name() -> Cow<'static, str> {
        "Direction".into()
    }
    fn inline_schema() -> bool {
        true
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "\"+x\", \"-x\", \"+y\", \"-y\", \"+z\", \"-z\" or [x, y, z].",
            "anyOf": [{ "type": "string" }, { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 }]
        })
    }
}

/// Round every number in a JSON tree to `dp` decimals (and −0 to 0): compact output for the model.
pub fn round_json(v: &mut Value, dp: i32) {
    match v {
        Value::Number(n) if n.is_f64() => {
            let r = round(n.as_f64().unwrap_or(0.0), dp);
            *v = json!(if r == 0.0 { 0.0 } else { r });
        }
        Value::Array(a) => a.iter_mut().for_each(|x| round_json(x, dp)),
        Value::Object(o) => o.values_mut().for_each(|x| round_json(x, dp)),
        _ => {}
    }
}

fn body_of(s: &Session, body: &Option<ObjRef>) -> Result<Option<Id>, String> {
    body.as_ref().map(|b| b.resolve(s)).transpose()
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TopologyArgs {
    /// Body id or name. Default: the part's current body (the result of the latest feature).
    #[serde(default)]
    pub body: Option<ObjRef>,
    /// List faces (default true).
    #[serde(default = "yes")]
    pub faces: bool,
    /// List edges (default true).
    #[serde(default = "yes")]
    pub edges: bool,
    /// Only faces of these kinds: plane, cylinder, cone, sphere, other.
    #[serde(default)]
    pub face_kind: Option<Vec<FaceKind>>,
    /// Only edges of these kinds: line, circle, arc, other.
    #[serde(default)]
    pub edge_kind: Option<Vec<EdgeKind>>,
    /// Only planar faces whose outward normal points this way (within `tol_deg`).
    #[serde(default)]
    pub facing: Option<DirArg>,
    /// Only straight edges running along this direction, either sense (within `tol_deg`).
    #[serde(default)]
    pub along: Option<DirArg>,
    /// Tolerance for `facing`/`along`, degrees (default 5).
    #[serde(default)]
    pub tol_deg: Option<f64>,
    /// Also list each face's edge ids and each edge's two face ids (default false).
    #[serde(default)]
    pub adjacency: bool,
    /// At most this many faces and this many edges (default 60).
    #[serde(default)]
    pub limit: Option<usize>,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectArgs {
    /// Body id or name. Default: the part's current body.
    #[serde(default)]
    pub body: Option<ObjRef>,
    /// A face selection to preview (give `faces` or `edges`).
    #[serde(default)]
    pub faces: Option<SelArg>,
    /// An edge selection to preview.
    #[serde(default)]
    pub edges: Option<SelArg>,
}

fn unit(d: [f64; 3]) -> Result<[f64; 3], String> {
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if !l.is_finite() || l < 1e-12 {
        return Err("a direction needs a length".into());
    }
    Ok([d[0] / l, d[1] / l, d[2] / l])
}

fn topology_json(t: &Topology, a: &TopologyArgs) -> Result<Value, String> {
    let cos = tol_deg(a.tol_deg.unwrap_or(5.0))?.to_radians().cos();
    let facing = a.facing.map(|d| unit(d.0)).transpose()?;
    let along = a.along.map(|d| unit(d.0)).transpose()?;
    let limit = a.limit.unwrap_or(60);
    let dot = |x: [f64; 3], y: [f64; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let mut out = json!({ "body": t.body, "faces_total": t.faces.len(), "edges_total": t.edges.len() });
    if !t.warnings.is_empty() {
        out["warnings"] = json!(t.warnings);
    }
    if a.faces {
        let faces: Vec<_> = t
            .faces
            .iter()
            .filter(|f| a.face_kind.as_ref().is_none_or(|k| k.contains(&f.kind)))
            .filter(|f| facing.is_none_or(|d| f.normal.is_some_and(|n| dot(n, d) >= cos)))
            .collect();
        if faces.len() > limit {
            out["faces_omitted"] = json!(faces.len() - limit);
        }
        out["faces"] = serde_json::to_value(&faces[..faces.len().min(limit)]).map_err(|e| e.to_string())?;
    }
    if a.edges {
        let edges: Vec<_> = t
            .edges
            .iter()
            .filter(|e| a.edge_kind.as_ref().is_none_or(|k| k.contains(&e.kind)))
            .filter(|e| {
                along.is_none_or(|d| {
                    let v = [e.b[0] - e.a[0], e.b[1] - e.a[1], e.b[2] - e.a[2]];
                    e.kind == EdgeKind::Line && unit(v).is_ok_and(|u| dot(u, d).abs() >= cos)
                })
            })
            .collect();
        if edges.len() > limit {
            out["edges_omitted"] = json!(edges.len() - limit);
        }
        out["edges"] = serde_json::to_value(&edges[..edges.len().min(limit)]).map_err(|e| e.to_string())?;
    }
    round_json(&mut out, 4);
    Ok(out)
}

pub fn tools() -> Vec<Tool> {
    vec![
        tool(
            "topology",
            "List the faces and edges of a body with persistent ids: faces (kind plane/cylinder/cone/sphere/other, \
             centroid, outward normal for planes, tessellation-based area (areas can be below analytic by up to ~2% on very small curved regions, including flat faces with curved boundaries, \
             as observed, not a guaranteed bound; use analytic dimensions for exact areas), axis+radius for cylinders/cones) and edges (kind \
             line/circle/arc/other, endpoints a/b, mid, length, centre/axis/radius for circles). Ids belong to ONE \
             body and are only valid after the latest feature: every feature makes a new body, so call topology \
             again after each one. Rows flagged ambiguous_id share a native name; warnings explain the workaround, and face selections matching these ids are refused. Use the filters (face_kind, edge_kind, facing, along, limit) to keep it short; \
             prefer descriptive selections (see `select`) over raw ids.",
            |st, a: TopologyArgs| {
                let s = st.doc()?;
                let body = body_of(s, &a.body)?;
                let t = s.topology(body, a.adjacency).map_err(err)?;
                topology_json(&t, &a)
            },
        ),
        tool(
            "select",
            "Preview what a face or edge selection resolves to on a body right now (the same rows as `topology`). \
             Use it to check a selection before fillet/chamfer/hole/shell/push_face. For inward corners, including \
             a boss/plate curved junction, \
             pass edges: {\"concave\": true}; combine with {\"and\": [{\"concave\": true}, {\"along\": \"y\"}]}. \
             Uncertain shallow signs or disagreement among five arc-length samples are omitted. Bare corner filters and positive `and` selections count only uncertain edges matching the other conditions; an `and` requiring both concave and convex matches no edge and reports no omission. Other compositions report the uncertain body total: corner filters treat those edges as neither concave nor convex, without claiming omission or result membership. \
             Analytic cone rims at slopes >=3 degrees are tested; 0.9 degrees may be omitted (native smoothness \
             threshold about 1.5 degrees). Fallback facet \
             normals can require about 20–30 degrees.",
            |st, a: SelectArgs| {
                let s = st.doc()?;
                let body = body_of(s, &a.body)?;
                let (el, sel) = match (&a.faces, &a.edges) {
                    (Some(f), None) => (Element::Faces, f.resolve(s)?),
                    (None, Some(e)) => (Element::Edges, e.resolve(s)?),
                    _ => return Err("give exactly one of `faces` or `edges`".into()),
                };
                let (body, ids) = s.select(body, el, &sel).map_err(err)?;
                let t = s.topology(Some(body), false).map_err(err)?;
                let mut out = match el {
                    Element::Faces => {
                        let rows: Vec<_> = ids.iter().filter_map(|i| t.faces.iter().find(|f| f.id == *i)).collect();
                        json!({ "body": body, "count": ids.len(), "faces": rows })
                    }
                    Element::Edges => {
                        let rows: Vec<_> = ids.iter().filter_map(|i| t.edges.iter().find(|e| e.id == *i)).collect();
                        json!({ "body": body, "count": ids.len(), "edges": rows })
                    }
                };
                if ids.is_empty() && el == Element::Edges {
                    if let Some(hint) = s.empty_corner_hint(body, &sel) {
                        out["hint"] = json!(hint);
                    }
                }
                if el == Element::Edges && sel.corner_filters() != (false, false) {
                    if let Some(note) = s.corner_omission_note(body, &sel).map_err(err)? {
                        out["note"] = json!(note);
                    }
                }
                round_json(&mut out, 4);
                Ok(out)
            },
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::round_json;
    use serde_json::json;

    #[test]
    fn rounding_nested_topology_preserves_large_finite_values() {
        // At four decimals the scaling factor is 10^4: 10^305 × 10^4 = 10^309,
        // beyond f64::MAX. Input spacing exceeds 10^-4, so it must stay unchanged.
        let mut value = json!({"faces": [{"area": 1e305, "centroid": [-1e305, -0.0, 1.23456]}]});
        round_json(&mut value, 4);
        assert_eq!(value["faces"][0]["area"], json!(1e305), "rounding must not turn a finite face area into null");
        assert_eq!(value["faces"][0]["centroid"][0], json!(-1e305));
        assert_eq!(value["faces"][0]["centroid"][1].as_f64().unwrap().to_bits(), 0.0f64.to_bits());
        // 1.23456 × 10^4 = 12345.6; round to 12346 and divide to get 1.2346.
        assert_eq!(value["faces"][0]["centroid"][2], json!(1.2346));
    }
}
