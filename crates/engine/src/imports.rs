//! Restore recipe-less imports whose saved B-rep is absent, using their embedded originals.
use crate::session::{AdvisoryWarning, NodeIssue};
use qymcad_core::feature::FeatureKind;
use qymcad_core::model::{Id, Project, SourceFile};
use qymcad_kernel::Shape;
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

/// Equivalent to pinned qymcad-testkit/src/lib.rs:54-84, limited to absent shapes. Unlike that
/// helper, isolate temporary files between sessions and hold the native gate. Apply the import
/// scale as the actual app does (qymcad/src/gui.rs:374-409). Stored B-reps remain authoritative.
pub(crate) fn restore_missing(project: &Project, shapes: &mut HashMap<Id, Shape>) -> Vec<AdvisoryWarning> {
    let mut by_source: HashMap<Id, Vec<(Id, u32, f64)>> = HashMap::new();
    for n in &project.timeline {
        if let FeatureKind::Import { body, source, solid, scale } = n.kind {
            by_source.entry(source).or_default().push((body, solid, scale));
        }
    }
    let mut warnings = Vec::new();
    for (source, bodies) in by_source {
        let restored = project.sources.iter().find(|sf| sf.id == source).ok_or_else(|| "source is missing".into()).and_then(read_source);
        let (mut solids, error) = match restored {
            Ok(solids) => (solids.into_iter().map(Some).collect::<Vec<_>>(), None),
            Err(error) => (Vec::new(), Some(error)),
        };
        for (body, solid, scale) in bodies {
            if shapes.contains_key(&body) && error.is_none() {
                continue;
            }
            let restored = solids
                .get_mut(solid as usize)
                .and_then(Option::take)
                .ok_or_else(|| error.clone().unwrap_or_else(|| format!("solid index {solid} is absent")))
                .and_then(|shape| {
                    if (scale - 1.0).abs() <= 1e-12 {
                        return Ok(shape);
                    }
                    qymcad_kernel::imports::scaled(&shape, scale).ok_or_else(|| format!("cannot apply import scale {scale}"))
                });
            match restored {
                Ok(shape) => {
                    shapes.insert(body, shape);
                }
                Err(error) => {
                    let name =
                        project.timeline.iter().find(|n| n.id == body).map(|n| crate::localization::name(&n.name)).unwrap_or_default();
                    let geometry =
                        if shapes.contains_key(&body) { "stored exact geometry is retained" } else { "exact geometry is unavailable" };
                    warnings.push(AdvisoryWarning::ImportSource(NodeIssue {
                        node: body,
                        message: format!("import body {body} `{name}` has an unavailable embedded source {source}: {error}; {geometry}; document data is retained"),
                        name,
                    }));
                }
            }
        }
    }
    warnings.sort_by_key(|w| w.issue().node);
    warnings
}

fn read_source(source: &SourceFile) -> Result<Vec<Shape>, String> {
    use std::os::unix::fs::OpenOptionsExt;
    if source.data.is_empty() {
        return Err("source is empty".into());
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let format = qymcad_kernel::ExactFormat::of_extension(&source.ext);
    let ext = match format {
        qymcad_kernel::ExactFormat::Step => "step",
        qymcad_kernel::ExactFormat::Iges => "igs",
    };
    for _ in 0..64 {
        let path =
            std::env::temp_dir().join(format!("qymcad-mcp-import-{}-{}.{ext}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let mut file = match std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("cannot stage source: {error}")),
        };
        let result = (|| {
            file.write_all(&source.data).map_err(|e| format!("cannot stage source: {e}"))?;
            let path = path.to_str().ok_or("temporary path is not UTF-8")?;
            let _gate = qymcad_kernel::kernel_gate();
            qymcad_kernel::exact_solids(format, path).map_err(|e| format!("cannot read source: {e}"))
        })();
        drop(file);
        let _ = std::fs::remove_file(&path);
        return result;
    }
    Err("cannot allocate a temporary source file".into())
}
