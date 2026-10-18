//! Vérifications des règles pures et de l'interface commune de stockage.
use std::io::Write;

use super::{
    storage::{probe, FileStore},
    types::*,
};

fn accepted(name: &str, data: &[u8], id: &str) -> bool {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(data).unwrap();
    detect_file(&mut file, name, &[id.to_string()]).is_ok()
}

#[test]
fn catalogue_et_signatures() {
    assert_eq!(CATALOG.len(), 19);
    assert!(accepted("plan.pdf", b"%PDF-1.7\n%%EOF", "pdf"));
    assert!(!accepted("plan.pdf", b"<html>faux PDF</html>", "pdf"));
    assert!(!accepted("plan.svg", b"<svg/>", "svg"));
    assert!(validate_settings(&["svg".into()], 50).is_err());
    assert!(validate_settings(&["pdf".into()], 0).is_err());
    assert!(validate_settings(&[], 201).is_err());
    assert!(!accepted("texte.txt", b"avant\0apres", "txt"));
    assert!(!accepted("texte.txt", b"<script>alert(1)</script>", "txt"));
    assert!(!accepted("faux.docx", b"PK\x03\x04faux conteneur", "docx"));
    assert!(!accepted(
        "faux.doc",
        b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1",
        "doc"
    ));
    assert!(accepted(
        "modele.step",
        b"ISO-10303-21;\nHEADER;\nENDSEC;",
        "step"
    ));
    assert!(accepted(
        "dessin.dxf",
        b"0\nSECTION\n2\nHEADER\n0\nENDSEC\n0\nEOF",
        "dxf"
    ));
    assert!(!accepted("dessin.dxf", b"Texte ordinaire", "dxf"));
    assert!(accepted("export.csv", b"nom;quantite\nVis;3\n", "csv"));
    assert!(!accepted("export.csv", b"texte sans separateur", "csv"));
    assert!(accepted(
        "photo.jpg",
        b"\xff\xd8\xff\xe0\0\x10JFIF\0",
        "jpeg"
    ));
    assert!(accepted("photo.png", b"\x89PNG\r\n\x1a\n", "png"));
    assert!(accepted("image.gif", b"GIF89a\x01\0\x01\0", "gif"));
    assert!(!accepted("image.gif", b"GIF89a\x01\0\x01\0", "pdf"));
    assert!(accepted("dessin.dwg", b"AC1027contenu", "dwg"));
}
#[test]
fn conteneurs_ooxml_inspectes() {
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name,data) in [("[Content_Types].xml","<Types><Override ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>"),("_rels/.rels","<Relationships/>"),("word/document.xml","<document/>")] {
        archive.start_file(name,options).unwrap(); archive.write_all(data.as_bytes()).unwrap();
    }
    let data = archive.finish().unwrap().into_inner();
    assert!(accepted("notice.docx", &data, "docx"));
    assert!(!accepted("notice.xlsx", &data, "xlsx"));
    assert!(accepted("notice.zip", &data, "zip"));
}
#[test]
fn noms_nettoyes() {
    assert_eq!(clean_name("C:\\dossier\\a:b?c\r\n.pdf"), "abc.pdf");
    assert_eq!(clean_name("../../notice.pdf"), "notice.pdf");
    assert_eq!(clean_name(&"é".repeat(210)).chars().count(), 200);
}
#[test]
fn secret_authentifie_et_aleatoire() {
    let files = FileStore::new(std::path::PathBuf::new(), [42; 32]);
    let a = files.encrypt("secret S3").unwrap();
    let b = files.encrypt("secret S3").unwrap();
    assert_ne!(a, b);
    assert_eq!(files.decrypt(&a).unwrap(), "secret S3");
    assert!(FileStore::new(std::path::PathBuf::new(), [41; 32])
        .decrypt(&a)
        .is_err());
    let changed = if a.starts_with("00") { "01" } else { "00" };
    assert!(files.decrypt(&format!("{changed}{}", &a[2..])).is_err());
    assert!(files.decrypt("invalide").is_err());
}
#[test]
fn formats_et_qr() {
    let mut format:LabelFormat=serde_json::from_value(serde_json::json!({"mode":"sheet","sheet":{"page":"A4","columns":3,"rows":8,"margin_top_mm":5,"margin_left_mm":5,"gap_horizontal_mm":0,"gap_vertical_mm":0},"single":{"width_mm":70,"height_mm":37},"qr_size_mm":20,"show_code":true,"show_name":true,"show_site":false,"show_production_line":false})).unwrap();
    assert!(format.validate().is_ok());
    format.sheet.columns = 20;
    assert!(format.validate().is_err());
    format.mode = "single".into();
    assert!(format.validate().is_ok());
    format.single.width_mm = 12.;
    assert!(format.validate().is_err());
    let svg = qr_svg("PRS-01").unwrap();
    let expected = qrcode::QrCode::with_error_correction_level(b"PRS-01", qrcode::EcLevel::M)
        .unwrap()
        .render::<qrcode::render::svg::Color>()
        .quiet_zone(true)
        .build();
    assert_eq!(svg, expected);
    assert_ne!(svg, qr_svg("PRS-02").unwrap());
    assert!(!svg.contains("PRS-01"));
}
#[tokio::test]
async fn stockage_memoire_meme_interface_que_s3() {
    let store = std::sync::Arc::new(object_store::memory::InMemory::new());
    probe(store.clone()).await.unwrap();
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(b"contenu S3").unwrap();
    super::storage::put_file(store.clone(), "2026/10/uuid", file.path())
        .await
        .unwrap();
    let data =
        object_store::ObjectStore::get(&*store, &object_store::path::Path::from("2026/10/uuid"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
    assert_eq!(&data[..], b"contenu S3");
}
#[tokio::test]
#[ignore = "Nécessite une instance MinIO de test et APEX_MINIO_ENDPOINT, APEX_MINIO_BUCKET, APEX_MINIO_ACCESS_KEY, APEX_MINIO_SECRET"]
async fn integration_minio() {
    let store = object_store::aws::AmazonS3Builder::new()
        .with_endpoint(std::env::var("APEX_MINIO_ENDPOINT").unwrap())
        .with_bucket_name(std::env::var("APEX_MINIO_BUCKET").unwrap())
        .with_region("us-east-1")
        .with_access_key_id(std::env::var("APEX_MINIO_ACCESS_KEY").unwrap())
        .with_secret_access_key(std::env::var("APEX_MINIO_SECRET").unwrap())
        .with_allow_http(true)
        .with_virtual_hosted_style_request(false)
        .build()
        .unwrap();
    probe(std::sync::Arc::new(store)).await.unwrap();
}
