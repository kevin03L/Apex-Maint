//! Catalogue sûr, noms de fichiers, formats d'étiquettes et QR.
use std::io::{Read, Seek};

use serde::{Deserialize, Serialize};

use crate::MaintError;

#[derive(Debug, Clone, Serialize)]
pub struct FileType {
    pub id: &'static str,
    pub extensions: &'static [&'static str],
    pub label: &'static str,
    pub disposition: &'static str,
    #[serde(skip)]
    pub mime: &'static str,
}

pub const CATALOG: &[FileType] = &[
    FileType {
        id: "pdf",
        extensions: &["pdf"],
        label: "PDF",
        disposition: "inline",
        mime: "application/pdf",
    },
    FileType {
        id: "jpeg",
        extensions: &["jpg", "jpeg"],
        label: "Image JPEG",
        disposition: "inline",
        mime: "image/jpeg",
    },
    FileType {
        id: "png",
        extensions: &["png"],
        label: "Image PNG",
        disposition: "inline",
        mime: "image/png",
    },
    FileType {
        id: "webp",
        extensions: &["webp"],
        label: "Image WebP",
        disposition: "inline",
        mime: "image/webp",
    },
    FileType {
        id: "gif",
        extensions: &["gif"],
        label: "Image GIF",
        disposition: "inline",
        mime: "image/gif",
    },
    FileType {
        id: "doc",
        extensions: &["doc"],
        label: "Word",
        disposition: "attachment",
        mime: "application/msword",
    },
    FileType {
        id: "docx",
        extensions: &["docx"],
        label: "Word OOXML",
        disposition: "attachment",
        mime: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    },
    FileType {
        id: "xls",
        extensions: &["xls"],
        label: "Excel",
        disposition: "attachment",
        mime: "application/vnd.ms-excel",
    },
    FileType {
        id: "xlsx",
        extensions: &["xlsx"],
        label: "Excel OOXML",
        disposition: "attachment",
        mime: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    },
    FileType {
        id: "odt",
        extensions: &["odt"],
        label: "Texte OpenDocument",
        disposition: "attachment",
        mime: "application/vnd.oasis.opendocument.text",
    },
    FileType {
        id: "ods",
        extensions: &["ods"],
        label: "Tableur OpenDocument",
        disposition: "attachment",
        mime: "application/vnd.oasis.opendocument.spreadsheet",
    },
    FileType {
        id: "csv",
        extensions: &["csv"],
        label: "CSV",
        disposition: "attachment",
        mime: "text/csv",
    },
    FileType {
        id: "txt",
        extensions: &["txt"],
        label: "Texte",
        disposition: "attachment",
        mime: "text/plain",
    },
    FileType {
        id: "dwg",
        extensions: &["dwg"],
        label: "Dessin DWG",
        disposition: "attachment",
        mime: "image/vnd.dwg",
    },
    FileType {
        id: "dxf",
        extensions: &["dxf"],
        label: "Dessin DXF",
        disposition: "attachment",
        mime: "image/vnd.dxf",
    },
    FileType {
        id: "step",
        extensions: &["step", "stp"],
        label: "Modèle STEP",
        disposition: "attachment",
        mime: "model/step",
    },
    FileType {
        id: "iges",
        extensions: &["iges", "igs"],
        label: "Modèle IGES",
        disposition: "attachment",
        mime: "model/iges",
    },
    FileType {
        id: "mp4",
        extensions: &["mp4"],
        label: "Vidéo MP4",
        disposition: "attachment",
        mime: "video/mp4",
    },
    FileType {
        id: "zip",
        extensions: &["zip"],
        label: "Archive ZIP",
        disposition: "attachment",
        mime: "application/zip",
    },
];

pub fn clean_name(name: &str) -> String {
    name.rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control() && !"\\/:*?\"<>|".contains(*c))
        .take(200)
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn validate_settings(allowed: &[String], max_size_mb: u32) -> Result<(), MaintError> {
    if !(1..=200).contains(&max_size_mb)
        || allowed.iter().any(|id| !CATALOG.iter().any(|t| t.id == id))
    {
        return Err(MaintError::BadRequest(
            "Types autorisés ou taille maximale invalides (1 à 200 Mo)".into(),
        ));
    }
    Ok(())
}

/// La signature est lue sur le temporaire ; les conteneurs sont inspectés sans extraction.
pub fn detect_file(
    file: &mut std::fs::File,
    name: &str,
    allowed: &[String],
) -> Result<(String, String), MaintError> {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let kind = CATALOG
        .iter()
        .find(|t| t.extensions.contains(&ext.as_str()) && allowed.iter().any(|id| id == t.id))
        .ok_or_else(|| MaintError::UnsupportedMediaType("Extension non autorisée".into()))?;
    file.rewind().map_err(io_error)?;
    let mut head = vec![0; 8192];
    let n = file.read(&mut head).map_err(io_error)?;
    head.truncate(n);
    let inferred = infer::get(&head);
    let signature = match kind.id {
        "docx" | "xlsx" | "odt" | "ods" | "zip" => valid_zip(file, kind.id),
        "doc" | "xls" => {
            // Le conteneur OLE et son flux propre doivent tous deux être présents.
            let stream = if kind.id == "doc" {
                "WordDocument"
            } else {
                "Workbook"
            };
            let marker: Vec<u8> = stream.encode_utf16().flat_map(u16::to_le_bytes).collect();
            head.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1") && contains_marker(file, &marker)?
        }
        "dwg" => {
            head.starts_with(b"AC10")
                && head
                    .get(4..6)
                    .is_some_and(|s| s.iter().all(u8::is_ascii_digit))
        }
        "txt" | "csv" | "dxf" | "step" | "iges" => valid_text(file, kind.id)?,
        _ => inferred.is_some_and(|t| t.mime_type() == kind.mime),
    };
    if !signature {
        return Err(MaintError::UnsupportedMediaType(
            "Le contenu ne correspond pas au type de fichier autorisé".into(),
        ));
    }
    Ok((ext, kind.mime.into()))
}

fn contains_marker(file: &mut std::fs::File, marker: &[u8]) -> Result<bool, MaintError> {
    file.rewind().map_err(io_error)?;
    let mut buffer = vec![0; 8192];
    let mut carry = Vec::new();
    loop {
        let n = file.read(&mut buffer).map_err(io_error)?;
        if n == 0 {
            return Ok(false);
        }
        carry.extend_from_slice(&buffer[..n]);
        if carry.windows(marker.len()).any(|w| w == marker) {
            return Ok(true);
        }
        let discard = carry.len().saturating_sub(marker.len() - 1);
        carry.drain(..discard);
    }
}

fn valid_zip(file: &mut std::fs::File, kind: &str) -> bool {
    if file.rewind().is_err() {
        return false;
    }
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return false;
    };
    match kind {
        "docx" | "xlsx" => {
            let main = if kind == "docx" {
                "word/document.xml"
            } else {
                "xl/workbook.xml"
            };
            if archive.by_name(main).is_err() || archive.by_name("_rels/.rels").is_err() {
                return false;
            }
            let Ok(content) = archive.by_name("[Content_Types].xml") else {
                return false;
            };
            let mut text = String::new();
            content.take(65537).read_to_string(&mut text).is_ok()
                && text.len() <= 65536
                && text.contains("<Types")
                && text.contains(if kind == "docx" {
                    "wordprocessingml"
                } else {
                    "spreadsheetml"
                })
        }
        "odt" | "ods" => {
            if archive.by_name("content.xml").is_err() {
                return false;
            }
            let Ok(mime) = archive.by_name("mimetype") else {
                return false;
            };
            let mut text = String::new();
            mime.take(256).read_to_string(&mut text).is_ok()
                && text
                    == if kind == "odt" {
                        "application/vnd.oasis.opendocument.text"
                    } else {
                        "application/vnd.oasis.opendocument.spreadsheet"
                    }
        }
        _ => true,
    }
}

fn valid_text(file: &mut std::fs::File, kind: &str) -> Result<bool, MaintError> {
    file.rewind().map_err(io_error)?;
    // Validation UTF-8 incrémentale, y compris une séquence à cheval sur deux blocs.
    let mut pending = Vec::new();
    let mut prefix = String::new();
    let mut buffer = [0; 8192];
    loop {
        let n = file.read(&mut buffer).map_err(io_error)?;
        if n == 0 {
            break;
        }
        if buffer[..n].contains(&0) {
            return Ok(false);
        }
        pending.extend_from_slice(&buffer[..n]);
        let valid = match std::str::from_utf8(&pending) {
            Ok(_) => pending.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => return Ok(false),
        };
        if prefix.len() < 8192 {
            prefix.push_str(std::str::from_utf8(&pending[..valid]).unwrap_or(""));
        }
        pending.drain(..valid);
    }
    if !pending.is_empty() || prefix.trim().is_empty() {
        return Ok(false);
    }
    let p = prefix.trim_start_matches('\u{feff}').trim_start();
    // Les formats actifs sont refusés aussi lorsqu'ils se présentent comme du texte.
    if p.starts_with('<') || p.starts_with("#!") {
        return Ok(false);
    }
    Ok(match kind {
        "dxf" => p
            .lines()
            .map(str::trim)
            .take(12)
            .collect::<Vec<_>>()
            .windows(2)
            .any(|w| w == ["0", "SECTION"]),
        "step" => p.starts_with("ISO-10303-21;"),
        "iges" => p.lines().any(|l| l.as_bytes().get(72) == Some(&b'S')),
        "csv" => p
            .lines()
            .next()
            .is_some_and(|l| l.contains([',', ';', '\t'])),
        _ => true,
    })
}

pub(super) fn io_error(e: std::io::Error) -> MaintError {
    MaintError::Internal(e.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SheetFormat {
    pub page: String,
    pub columns: u32,
    pub rows: u32,
    pub margin_top_mm: f64,
    pub margin_left_mm: f64,
    pub gap_horizontal_mm: f64,
    pub gap_vertical_mm: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SingleFormat {
    pub width_mm: f64,
    pub height_mm: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelFormat {
    pub mode: String,
    pub sheet: SheetFormat,
    pub single: SingleFormat,
    pub qr_size_mm: f64,
    pub show_code: bool,
    pub show_name: bool,
    pub show_site: bool,
    pub show_production_line: bool,
}
impl LabelFormat {
    pub fn validate(&self) -> Result<(), MaintError> {
        let s = &self.sheet;
        let values = [
            s.margin_top_mm,
            s.margin_left_mm,
            s.gap_horizontal_mm,
            s.gap_vertical_mm,
            self.single.width_mm,
            self.single.height_mm,
            self.qr_size_mm,
        ];
        let (page_w, page_h) = match s.page.as_str() {
            "A4" => (210., 297.),
            "Letter" => (215.9, 279.4),
            _ => (0., 0.),
        };
        let valid = values
            .iter()
            .all(|n| n.is_finite() && *n >= 0. && *n <= 500.)
            && (1..=20).contains(&s.columns)
            && (1..=50).contains(&s.rows)
            && (10.0..=100.0).contains(&self.qr_size_mm)
            && (10.0..=500.0).contains(&self.single.width_mm)
            && (10.0..=500.0).contains(&self.single.height_mm);
        let (w, h) = match self.mode.as_str() {
            "sheet" => (
                (page_w
                    - 2. * s.margin_left_mm
                    - f64::from(s.columns.saturating_sub(1)) * s.gap_horizontal_mm)
                    / f64::from(s.columns.max(1)),
                (page_h
                    - 2. * s.margin_top_mm
                    - f64::from(s.rows.saturating_sub(1)) * s.gap_vertical_mm)
                    / f64::from(s.rows.max(1)),
            ),
            "single" => (self.single.width_mm, self.single.height_mm),
            _ => (0., 0.),
        };
        let lines = [
            self.show_code,
            self.show_name,
            self.show_site,
            self.show_production_line,
        ]
        .into_iter()
        .filter(|b| *b)
        .count();
        if !valid
            || page_w == 0.
            || w < self.qr_size_mm + 2.
            || h < self.qr_size_mm + 2. + lines as f64 * 3.
        {
            return Err(MaintError::BadRequest("Format d'étiquette invalide : le QR et les textes doivent tenir dans l'étiquette et la page".into()));
        }
        Ok(())
    }
}

pub fn qr_svg(code: &str) -> Result<String, MaintError> {
    let qr = qrcode::QrCode::with_error_correction_level(code.as_bytes(), qrcode::EcLevel::M)
        .map_err(|_| MaintError::BadRequest("Code trop long pour un QR".into()))?;
    // Le moteur SVG ajoute une zone calme de quatre modules, sans aucun texte utilisateur.
    Ok(qr
        .render::<qrcode::render::svg::Color>()
        .quiet_zone(true)
        .build())
}
