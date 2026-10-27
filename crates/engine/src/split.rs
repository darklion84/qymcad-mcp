//! Fixed-count native body splitting and native output-body names.
use crate::{BaseName, BodyInfo, Element, Error, Id, Num, PlaneRef, Rebuild, Result, Sel, Session};
use qymcad_core::{
    feature::PlaneAt,
    model::{FeatNumber, PlainNumber},
};
use serde::Serialize;
use std::collections::HashSet;

/// Arguments for a fixed-count split of one current body.
#[derive(Clone, Debug)]
pub struct SplitOptions {
    /// Stock body; defaults to the current tip. Use an explicit body after splitting.
    pub body: Option<Id>,
    /// Base plane, datum plane or planar body face.
    pub plane: PlaneRef,
    /// Signed offset, mm: XY +Z, XZ +Y, YZ +X; datum/face along its own normal.
    pub offset: Num,
    /// Timeline feature name.
    pub name: Option<String>,
    /// One distinct native body name per measured piece; defaults to feature name plus 1..n.
    pub piece_names: Option<Vec<String>>,
}

/// Split feature, its independently addressable pieces and rebuild diagnostics.
#[derive(Debug, Serialize)]
pub struct SplitResult {
    /// First output body; also the feature id for existing creation/history clients.
    pub body: Id,
    /// Timeline feature id.
    pub feature: Id,
    /// Feature name.
    pub name: String,
    /// Output bodies in the native plane-normal ordering.
    pub pieces: Vec<BodyInfo>,
    /// Evaluated signed plane offset, mm.
    pub offset: f64,
    /// Current document bodies and rebuild diagnostics.
    pub rebuild: Rebuild,
}

impl Session {
    /// Split a current body by a plane, fixing the measured output count and native piece ids at creation.
    /// Later count-changing edits fail through the normal atomic rebuild path; recreate the split to change count.
    /// Precondition: the caller must not hold the kernel gate (`ask::split_pieces` acquires it).
    pub fn split(&mut self, options: &SplitOptions) -> Result<SplitResult> {
        self.ensure_topology()?;
        let ((ids, names, offset), mut rebuild) = self.atomic(|s| {
            let src = s.source_body(options.body)?;
            let offset = options.offset.eval(&s.p.param_map())?;
            let (plane, datum, face, origin, normal) = match &options.plane {
                PlaneRef::Base(base) => {
                    let (code, normal) = match base {
                        BaseName::XY => (0, [0.0, 0.0, 1.0]),
                        BaseName::XZ => (1, [0.0, 1.0, 0.0]),
                        BaseName::YZ => (2, [1.0, 0.0, 0.0]),
                    };
                    (code, 0, None, [0.0; 3], normal)
                }
                PlaneRef::Plane(id) => {
                    let p = s.p.planes.iter().find(|p| p.id == *id).ok_or_else(|| Error::NotFound(format!("datum plane {id}")))?;
                    (0, *id, None, p.origin, p.normal)
                }
                PlaneRef::Face { body, face } => {
                    s.resolve_sel(*body, Element::Faces, &Sel::Ids(vec![*face]))?;
                    if !s.face_is_planar(*body, *face) {
                        return Err(Error::Invalid(format!("split face {face} of body {body} is not planar")));
                    }
                    let f =
                        s.p.regen_faces
                            .get(body)
                            .into_iter()
                            .flatten()
                            .find(|f| f.id == *face)
                            .ok_or_else(|| Error::NotFound(format!("face {face} of body {body}")))?;
                    let key = crate::topology::face_key(f);
                    let (origin, normal) = s.p.resolve_face(*body, &key);
                    (0, 0, Some((*body, key)), origin, normal)
                }
            };
            let magnitude = normal.iter().map(|v| v * v).sum::<f64>().sqrt();
            if !magnitude.is_finite() || magnitude < 1e-12 || origin.iter().any(|v| !v.is_finite()) {
                return Err(Error::Invalid("split plane needs a finite origin and nonzero finite normal".into()));
            }
            let normal = normal.map(|v| v / magnitude);
            let at = PlaneAt { origin: std::array::from_fn(|i| origin[i] + offset * normal[i]), normal };
            let shape = s.shapes.get(&src).ok_or_else(|| Error::NotFound(format!("body {src} has no built shape")))?;
            // This public question copies the source and owns its gate. Never wrap it in an outer gate.
            let count = qymcad_kernel::ask::split_pieces(shape, at)
                .filter(|n| *n >= 2)
                .ok_or_else(|| Error::Invalid("split plane cuts nothing: it must divide the body into at least two pieces".into()))?;
            if options.piece_names.as_ref().is_some_and(|names| names.len() != count) {
                return Err(Error::Invalid(format!("piece_names needs exactly {count} names for the measured pieces")));
            }
            let ids = s.p.add_split_body(src, plane, datum, offset, count);
            let feature = ids[0]; // count>=2 checked before the native adder's assertion.
            if let Some((body, key)) = face {
                s.p.set_op_face(feature, body, key);
            }
            let text = options.offset.expr().unwrap_or_else(|| offset.to_string());
            s.p.write_feat_formula(&FeatNumber { node: feature, key: "offset" }, &text, PlainNumber::Stands)
                .map_err(|e| Error::Expr(format!("split offset: {e:?}")))?;
            s.set_node_name(feature, options.name.as_deref());
            let names =
                options.piece_names.clone().unwrap_or_else(|| (1..=count).map(|i| format!("{} {i}", s.node_name(feature))).collect());
            let mut seen = HashSet::new();
            for name in &names {
                if name.trim().is_empty() || name != name.trim() || !seen.insert(name) {
                    return Err(Error::Invalid("piece_names must be distinct, nonblank names without surrounding whitespace".into()));
                }
                if s.resolve(name).is_ok() {
                    return Err(Error::Invalid(format!("piece name `{name}` already names an object; choose distinct piece_names")));
                }
            }
            Ok((ids, names, offset))
        })?;
        // Rebuild has now created every native mesh. Names live on Body, not on the feature node.
        for (&id, name) in ids.iter().zip(names) {
            if let Some(mi) = self.p.mesh_index(id) {
                self.p.set_mesh_name(mi, name);
            }
        }
        rebuild.bodies = self.result_bodies();
        let pieces = rebuild.bodies.iter().filter(|b| ids.contains(&b.id)).cloned().collect();
        let feature = ids[0];
        Ok(SplitResult { body: feature, feature, name: self.node_name(feature), pieces, offset, rebuild })
    }
}
