//! Sweep and loft: solids from sketch profiles moved along a path, or passed through ordered sections. Atomic
//! like every feature. Neither has a size of its own: the profile, the path and the sections are sketches, so
//! their dimensions and the planes they stand on carry the formulas.

use crate::error::{Error, Result};
use crate::features::Op;
use crate::session::{Rebuild, Session};
use crate::topology::{Element, Sel};
use qymcad_core::feature::EdgePath;
use qymcad_core::model::Id;
use qymcad_core::refs::Ref;

/// What a sweep runs along.
#[derive(Clone, Debug, PartialEq)]
pub enum SweepPath {
    /// A contour of another sketch, open or closed. `profile`: its contour id; default the only path contour.
    Sketch {
        /// The path sketch.
        sketch: Id,
        /// The path contour; required when the sketch has more than one.
        profile: Option<Id>,
    },
    /// Edges of a body that join end to end into one chain, kept by their persistent names.
    Edges {
        /// The body (default: the part's current body).
        body: Option<Id>,
        /// The edges: ids or a description.
        edges: Sel,
    },
}

/// Arguments of `Session::sweep`.
#[derive(Clone, Debug, PartialEq)]
pub struct Sweep {
    /// The profile sketch.
    pub sketch: Id,
    /// Closed profile contours; default every top-level closed contour.
    pub profiles: Option<Vec<Id>>,
    /// The path.
    pub path: SweepPath,
    /// Add (creates the first body), cut, intersect, or a new body.
    pub op: Op,
    /// Body to modify (default: the current body). Not allowed with `Op::NewBody`.
    pub target: Option<Id>,
    /// Name for the new feature.
    pub name: Option<String>,
}

/// One section of a loft.
#[derive(Clone, Debug, PartialEq)]
pub struct LoftSection {
    /// The section sketch.
    pub sketch: Id,
    /// Its closed contour; required when the sketch has more than one.
    pub profile: Option<Id>,
}

/// Arguments of `Session::loft`.
#[derive(Clone, Debug, PartialEq)]
pub struct Loft {
    /// At least two sections, in the order the loft passes through them.
    pub sections: Vec<LoftSection>,
    /// Straight faces between sections instead of smooth ones.
    pub ruled: bool,
    /// Add (creates the first body), cut, intersect, or a new body.
    pub op: Op,
    /// Body to modify (default: the current body). Not allowed with `Op::NewBody`.
    pub target: Option<Id>,
    /// A sheet through the sections instead of a solid; only with `Op::NewBody`.
    pub surface: bool,
    /// Name for the new feature.
    pub name: Option<String>,
}

impl Session {
    /// Sweep closed profile contours along a sketch path or a chain of body edges, as the app does
    /// (`add_sweep_multi_op` / `add_sweep_along`). Returns the new body id.
    pub fn sweep(&mut self, a: &Sweep) -> Result<(Id, Rebuild)> {
        if matches!(a.path, SweepPath::Edges { .. }) {
            self.ensure_topology()?;
        }
        self.atomic(|s| {
            let profiles = s.closed_profiles(a.sketch, a.profiles.as_deref())?;
            let src = s.op_source(a.op, a.target)?;
            let (code, src) = match src {
                Some(src) => (op_code(a.op), src),
                None => (1, 0),
            };
            let id = match &a.path {
                SweepPath::Sketch { sketch, profile } => {
                    if *sketch == a.sketch {
                        return Err(Error::Invalid("the path must be a contour of another sketch than the profile".into()));
                    }
                    s.sketch_si(*sketch)?;
                    let paths = s.p.sweep_path_contours(*sketch);
                    let path = match profile {
                        Some(c) if paths.contains(c) => *c,
                        Some(c) => return Err(Error::NotFound(format!("path contour {c} in sketch {sketch} (candidates {paths:?})"))),
                        None => match paths.as_slice() {
                            [one] => *one,
                            [] => return Err(Error::Invalid(format!("sketch {sketch} has no contour to sweep along"))),
                            many => {
                                return Err(Error::Invalid(format!(
                                    "sketch {sketch} has several path contours {many:?}; give `profile` in `path`"
                                )))
                            }
                        },
                    };
                    s.p.add_sweep_multi_op(a.sketch, profiles, *sketch, path, src, code)
                }
                SweepPath::Edges { body, edges } => {
                    let body = s.source_body(*body)?;
                    let mut ids = s.resolve_sel(body, Element::Edges, edges)?;
                    ids.sort_unstable();
                    ids.dedup();
                    if ids.is_empty() {
                        return Err(Error::Invalid(format!("the edge selection matched no edge of body {body}")));
                    }
                    let shape = s.shapes.get(&body).ok_or_else(|| Error::Invalid(format!("body {body} is not built")))?;
                    // Self-gating kernel door: no kernel gate is held here (F-073).
                    qymcad_kernel::ask::path_edges_join(shape, &ids)
                        .map_err(|_| Error::Invalid(format!("edges {ids:?} of body {body} do not join end to end into one chain")))?;
                    s.p.add_sweep_along(a.sketch, profiles, EdgePath { body, edges: Ref::picks(&ids) }, src, code)
                }
            };
            s.set_node_name(id, a.name.as_deref());
            Ok(id)
        })
    }

    /// Loft through closed section contours in order, as the app does (`add_loft`). Returns the new body id.
    pub fn loft(&mut self, a: &Loft) -> Result<(Id, Rebuild)> {
        self.atomic(|s| {
            if a.sections.len() < 2 {
                return Err(Error::Invalid(format!("a loft needs at least two sections, got {}", a.sections.len())));
            }
            if a.surface && a.op != Op::NewBody {
                return Err(Error::Invalid("a surface loft is a separate sheet body: use op new_body".into()));
            }
            let mut sketches = Vec::new();
            let mut contours = Vec::new();
            for section in &a.sections {
                // Every closed contour is a possible section, nested ones included (a loft takes one region).
                s.sketch_si(section.sketch)?;
                let candidates = s.p.sweep_profile_contours(section.sketch);
                let contour = match section.profile {
                    Some(c) => s.closed_profiles(section.sketch, Some(&[c]))?[0],
                    None => match candidates.as_slice() {
                        [one] => *one,
                        [] => return Err(Error::Invalid(format!("sketch {} has no closed contour", section.sketch))),
                        many => {
                            return Err(Error::Invalid(format!(
                                "sketch {} has several closed contours {many:?}; give `profile` for this section",
                                section.sketch
                            )))
                        }
                    },
                };
                if sketches.last() == Some(&section.sketch) && contours.last() == Some(&contour) {
                    return Err(Error::Invalid(format!("section contour {contour} is given twice in a row")));
                }
                sketches.push(section.sketch);
                contours.push(contour);
            }
            let (code, src) = match s.op_source(a.op, a.target)? {
                Some(src) => (op_code(a.op), src),
                None => (1, 0),
            };
            let id = s.p.add_loft(sketches, contours, a.ruled, src, code, a.surface);
            s.set_node_name(id, a.name.as_deref());
            Ok(id)
        })
    }

    /// The closed contours of `sketch` to use as profiles: `given` checked to be closed contours of this sketch,
    /// or by default every top-level closed contour.
    fn closed_profiles(&self, sketch: Id, given: Option<&[Id]>) -> Result<Vec<Id>> {
        self.sketch_si(sketch)?;
        let closed = self.p.sweep_profile_contours(sketch);
        let ids = match given {
            Some(ids) if !ids.is_empty() => {
                if let Some(bad) = ids.iter().find(|c| !closed.contains(c)) {
                    return Err(Error::NotFound(format!("closed contour {bad} in sketch {sketch} (closed contours {closed:?})")));
                }
                ids.to_vec()
            }
            _ => self.root_contours(sketch)?.into_iter().filter(|c| closed.contains(c)).collect(),
        };
        if ids.is_empty() {
            return Err(Error::Invalid(format!("sketch {sketch} has no closed contour")));
        }
        Ok(ids)
    }

    /// The body an operation works on: `None` for a new body (or the first add), refusing a target for a new
    /// body and a cut or intersection with nothing to work on.
    fn op_source(&self, op: Op, target: Option<Id>) -> Result<Option<Id>> {
        let src = match (target, op) {
            (Some(_), Op::NewBody) => return Err(Error::Invalid("op new_body makes a separate body; omit `target`".into())),
            (Some(t), _) => Some(self.source_body(Some(t))?),
            (None, Op::NewBody) => None,
            (None, _) => self.tip_body(),
        };
        match (op, src) {
            (Op::NewBody, _) => Ok(None),
            (Op::Cut | Op::Intersect, None) => Err(Error::Invalid("there is no body to cut or intersect yet".into())),
            (_, src) => Ok(src),
        }
    }
}

/// QymCAD's boolean codes: 0 cut, 1 union, 2 intersection.
fn op_code(op: Op) -> u8 {
    match op {
        Op::Cut => 0,
        Op::Intersect => 2,
        _ => 1,
    }
}
