//! Elle çalıştırılır (ağ): cargo test --test ag_canli -- --ignored
//! Sabit köke HTTPS ile ulaşılabildiğini ve olmayan dosyada 404 → None döndüğünü denetler.
#[test]
#[ignore]
fn kok_adrese_ulasiliyor() {
    let a = armonia_guncelle::ag::ajan();
    let r = armonia_guncelle::ag::getir(&a, "olmayan-dosya-deneme.json", 1024);
    println!("{r:?}");
    assert!(matches!(r, Ok(None)), "{r:?}");
}
