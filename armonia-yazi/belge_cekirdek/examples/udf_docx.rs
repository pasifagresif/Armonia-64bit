//! UDF → DOCX çevirici (deneme): cargo run --example udf_docx -- <girdi.udf> <çıktı.docx>. Claude, 2026-11-23.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let b = belge_cekirdek::udf::oku(&std::fs::read(&a[1]).expect("okunamadı")).expect("UDF değil").belge;
    std::fs::write(&a[2], belge_cekirdek::docx_yaz::yaz(&b).expect("yazılamadı")).expect("yazılamadı");
    println!("yazıldı: {}", a[2]);
}
