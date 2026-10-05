//! qymcad-mcp: the MCP stdio server. Protocol and tool definitions only; all modelling is in `qymcad-engine`.

pub mod tools;
pub mod transport;

use std::path::PathBuf;

/// Guidance sent to the client in `initialize` (`instructions`): how to model with these tools.
pub fn instructions(app_note: &str) -> String {
    format!(
        "qymcad-mcp builds native, parametric QymCAD parts (.qcad) headlessly. Units: mm, degrees.\n\
         Workflow: doc_new -> param_set for every dimension the user may change (lowercase names) -> \
         sketch_create on \"XY\"/\"XZ\"/\"YZ\", a datum plane or a face -> sketch_add rect/circle using expressions \
         like \"w\" or \"-hx\" -> extrude (op add/cut/intersect; direction relative to the sketch normal: XY +Z, \
         XZ -Y, YZ +X) -> doc_info to check bodies (volume, bbox), errors, warnings -> doc_save to a .qcad path.\n\
         A pocket from the top: plane_offset {{base: \"XY\", dist: \"t\"}}, sketch on {{plane: id}}, extrude op cut \
         direction reverse. Holes: circles inside a rectangle in the same sketch become through holes when the \
         rectangle is extruded.\n\
         render (view iso/top/front/...) returns a picture to check the shape; export writes STEP/STL/3MF/GLB/OBJ.\n\
         Every feature is atomic: on error nothing changes and the message says why. Objects can be referred to \
         by id or by the name given at creation.\n\
         The user opens the file in the QymCAD app with File > Open (QymCAD {ver}; other releases may not read it). {app_note}",
        ver = qymcad_engine::QYMCAD_VERSION,
    )
}

/// Compare the installed QymCAD.app with the pinned release (ADR 0003). Returns a note for the agent.
///
/// The bundle's Info.plist says only `0.1.0`; the release tag (`v0.1.0-dev.YYYYMMDD`) is embedded in the
/// executable (FINDINGS F-018), so we look for it there.
pub fn app_version_note() -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let candidates = [PathBuf::from("/Applications/QymCAD.app"), home.join("Applications/QymCAD.app")];
    for app in candidates {
        let Ok(plist) = std::fs::read_to_string(app.join("Contents/Info.plist")) else { continue };
        let exe = plist_string(&plist, "CFBundleExecutable").unwrap_or_else(|| "qymcad".into());
        let Ok(bin) = std::fs::read(app.join("Contents/MacOS").join(exe)) else { continue };
        let found = embedded_release(&bin);
        let want = qymcad_engine::QYMCAD_VERSION;
        return match found {
            Some(v) if v == want => format!("Installed {} is {v}: matches.", app.display()),
            Some(v) => format!(
                "WARNING: installed {} is {v} but this server writes files for {want}; tell the user files may not open.",
                app.display()
            ),
            None => format!("Could not read the release of {}; this server writes files for {want}.", app.display()),
        };
    }
    "QymCAD.app was not found in /Applications or ~/Applications.".to_string()
}

/// The first `v<major>.<minor>.<patch>-dev.<8 digits>` tag embedded in an executable, without the `v`.
fn embedded_release(bin: &[u8]) -> Option<String> {
    let needle = b"-dev.";
    let mut from = 0;
    while let Some(pos) = bin[from..].windows(needle.len()).position(|w| w == needle) {
        let at = from + pos;
        let digits = bin.get(at + needle.len()..at + needle.len() + 8)?;
        let start = bin[..at].iter().rposition(|&c| c == b'v').filter(|&v| at - v <= 12);
        if let (true, Some(v)) = (digits.iter().all(u8::is_ascii_digit), start) {
            let head = &bin[v + 1..at];
            if !head.is_empty() && head.iter().all(|c| c.is_ascii_digit() || *c == b'.') {
                return std::str::from_utf8(&bin[v + 1..at + needle.len() + 8]).ok().map(str::to_string);
            }
        }
        from = at + 1;
    }
    None
}

/// The `<string>` following `<key>key</key>` in an XML plist.
fn plist_string(plist: &str, key: &str) -> Option<String> {
    let k = plist.find(&format!("<key>{key}</key>"))?;
    let rest = &plist[k..];
    let a = rest.find("<string>")? + "<string>".len();
    let b = rest[a..].find("</string>")?;
    Some(rest[a..a + b].trim().to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_a_plist_string() {
        let p = "<dict><key>CFBundleName</key><string>QymCAD</string><key>CFBundleShortVersionString</key>\n<string>0.1.0-dev.20261001</string></dict>";
        assert_eq!(super::plist_string(p, "CFBundleShortVersionString").as_deref(), Some("0.1.0-dev.20261001"));
        assert_eq!(super::plist_string(p, "Missing"), None);
    }

    #[test]
    fn finds_the_embedded_release_tag() {
        let bin = b"\x00junk-dev.x\x00assertion failed: lenv0.1.0-dev.20261001d2949f0a72026-10-01macos";
        assert_eq!(super::embedded_release(bin).as_deref(), Some("0.1.0-dev.20261001"));
        assert_eq!(super::embedded_release(b"nothing here"), None);
    }
}
