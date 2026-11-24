# İcra Takip Süreçleri → Dosya Listesi sekmesi: sahte uçlarla tarayıcı denemesi (20.11.2026).
# Çalıştırma: python araclar/icra_dosya_listesi_test.py uyap_core/enjekte/icra.js <çıktı klasörü> [uyap_core/enjekte/tema.css]
import json, sys, os, urllib.parse, datetime
from playwright.sync_api import sync_playwright

JS = sys.argv[1]
CIKTI = sys.argv[2]
TEMA = sys.argv[3] if len(sys.argv) > 3 else None
os.makedirs(CIKTI, exist_ok=True)

DAIRELER = ["İstanbul 11. İcra Dairesi", "İstanbul 2. İcra Dairesi", "Çorum 1. İcra Dairesi", "Cizre 1. İcra Dairesi",
            "Ankara Banka Alacakları İcra Dairesi", "İstanbul Abonelik Sözleşmeleri İcra Dairesi", "Ankara 3. İcra Dairesi"]

def satir(i, birim, acik=True):
    return {"dosya": i, "birim": birim, "dosya_no": "2026/%d" % (100 + i), "dosya_tur": "İcra Dosyası", "yil": 2026,
            "dosya_turu": "İcra Dosyası (MTS'den)" if i % 3 == 0 else "İcra Dosyası", "mts_kaynakli": i % 3 == 0,
            "durum": "Açık" if acik else "Kapalı (Haricen Tahsil - 12.03.2026)", "acik": acik,
            "acilis_tarihi": "2026-0%d-10" % (i % 9 + 1), "kapanis_tarihi": None if acik else "2026-03-12",
            "kapanis_sebebi": "" if acik else "Haricen Tahsil",
            "alacaklilar": ["X BANKASI A.Ş."] if i % 2 else ["Y A.Ş.", "Z A.Ş."], "borclular": ["AHMET YILMAZ", "MEHMET YILMAZ", "ALİ KAYA"][: 1 + i % 3],
            "rol": ["alacakli", "borclu", "diger_alacakli", "ucuncu", ""][i % 5],
            "rol_etiket": ["Alacaklı vekili", "Borçlu vekili", "Diğer haciz/rehin alacaklısı vekili", "3. kişi vekili", ""][i % 5],
            "rol_taraflar": [] if i % 5 == 4 else (["X BANKASI A.Ş."] if i % 5 == 0 else ["AHMET YILMAZ", "MEHMET YILMAZ"] if i % 5 == 1 else ["Z A.Ş."]),
            "takip_turu": "İlamsız", "takip_yolu": "Genel Haciz Yoluyla", "takip_sekli": "Örnek 7"}

SORGULAR, POSTLAR, TAHMINLER, KSORGULAR = [], [], [], []
KS_GRUPLAR = [("kesinlestirilebilir", "Kesinleştirilebilir"), ("talep_sonucsuz", "Talep gönderildi, kesinleşme görülmedi"),
              ("talep_gonderildi", "Talep gönderildi"), ("sure_isliyor", "Süre işliyor"), ("bekleniyor", "Tebliğ / mazbata bekleniyor"),
              ("uyap_kesinlesti", "UYAP'ta kesinleşti"), ("durdu", "İtiraz / durduruldu")]

def ks_satir(i):
    grup = ["kesinlestirilebilir", "kesinlestirilebilir", "sure_isliyor", "bekleniyor", "uyap_kesinlesti"][i - 1]
    return {"bag": 100 + i, "dosya": 10 + i, "birim": DAIRELER[i - 1], "dosya_no": "2026/%d" % (500 + i), "acilis_tarihi": "2026-09-01",
            "alacaklilar": ["X BANKASI A.Ş."], "borclu": ["AHMET YILMAZ", "AYŞE KAYA", "VİVEN LTD.", "ALİ VELİ", "CAN ER"][i - 1],
            "takip_turu": "İlamsız Takip", "takip_yolu": "Genel Haciz Yoluyla Takip",
            "asama": "teblig" if i > 2 else "kesinlesti", "asama_etiket": "Tebliğ", "asama_tarihi": "2026-10-01",
            "teblig": "teblig_edildi", "teblig_tarihi": "2026-10-01", "kesinlesme": "kesinlesti" if i <= 2 else "kesinlesme_bekliyor",
            "kesinlesme_etiket": "Kesinleşti" if i <= 2 else "Kesinleşme Bekliyor", "kesinlesme_tarihi": None if i == 4 else "2026-11-%02d" % (10 + 5 * i),
            "elle_tarih": None, "uyap": True if i == 5 else None, "uyap_zamani": "2026-11-21T20:15" if i == 5 else None,
            "uyap_gunu": "2026-11-21" if i == 5 else None, "talep": None, "mazbata": "dondu" if i != 4 else "donmedi",
            "mazbata_etiket": "Mazbata döndü" if i != 4 else "Mazbata dönmedi", "barkod": "4007663523293",
            "grup": grup, "grup_etiket": dict(KS_GRUPLAR)[grup], "kalan_gun": 3 if grup == "sure_isliyor" else None,
            "yeniden": "uygun" if i == 4 else "", "yeniden_etiket": "21/2 şerhli tebliğe uygun" if i == 4 else "—",
            "mernis": {"kategori": "uygun", "etiket": "21/2 şerhli tebliğe uygun", "adres": "BOSTANLI MAH. 1720 SK. No:5 KARŞIYAKA/ İZMİR",
                       "teblig_adresi": "Bostanlı Mah. 1720 Sk. No:5 Karşıyaka/ İzmir", "aciklama": "Ödeme emri Mernis adresine çıkmış, iade dönmüş",
                       "zaman": "2026-11-23T19:05"} if i == 4 else None, "yeniden_talep": None}
COK = {"satir": 0}   # > 0: liste bu kadar satır (kaydırma denemesi)
# Dosya listesi 2 saat önce alındı (Tüm verileri güncelle: "Hayır, yalnız eksikler" öne çıkar).
LISTE_ZAMANI = (datetime.datetime.now() - datetime.timedelta(hours=2)).strftime("%Y-%m-%dT%H:%M:%S")

def secenek(k, adetler, etiket=None):
    return [{"deger": d, "etiket": (etiket or (lambda x: x))(d), "adet": n} for d, n in adetler]

AYLAR = ["Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül", "Ekim", "Kasım", "Aralık"]

def karsila(route):
    u = urllib.parse.urlparse(route.request.url)
    uc = u.path.split("/__cu__/veri/")[1]
    q = dict(urllib.parse.parse_qsl(u.query, keep_blank_values=True))
    if uc == "dosyalar":
        SORGULAR.append(q)
        durum = q.get("durum", "")
        if COK["satir"]:
            satirlar = [satir(i, DAIRELER[i % len(DAIRELER)], acik=(durum != "kapali")) for i in range(1, COK["satir"] + 1)]
        else:
            satirlar = [satir(i, b, acik=(durum != "kapali")) for i, b in enumerate(DAIRELER, 1)]
        veri = {"toplam": len(satirlar), "sayfa": 1, "boyut": 100, "satirlar": satirlar, "secenek_siniri": 300, "avukat_bilinmiyor": False, "genel_toplam": 5312,
                "secenekler": {"birim": [{"deger": b, "etiket": b, "adet": 1} for b in DAIRELER],
                               "rol": [{"deger": "alacakli", "etiket": "Alacaklı vekili", "adet": 2}, {"deger": "borclu", "etiket": "Borçlu vekili", "adet": 2}],
                               "alacakli": [], "borclu": [], "dosya_no": [], "takip_turu": [], "takip_yolu": [], "takip_sekli": [],
                               "dosya_tur": secenek("dosya_tur", [("İcra Dosyası", 5), ("İcra Dosyası (MTS'den)", 2)]),
                               "yil": secenek("yil", [("2026", 4), ("2024", 1), ("2025", 2)]),
                               "acilis_yil": secenek("acilis_yil", [("2026", 6), ("2025", 1)]),
                               "acilis_ay": secenek("acilis_ay", [("3", 2), ("10", 5)], lambda a: AYLAR[int(a) - 1]),
                               "kapanis_yil": secenek("kapanis_yil", [("2026", 7)]),
                               "kapanis_ay": secenek("kapanis_ay", [("3", 7)], lambda a: AYLAR[int(a) - 1])}}
    elif uc == "dosya":
        i = int(q.get("id", "1"))
        veri = {"kunye": {"id": i, "dosya_no": "2026/%d" % (100 + i), "birim": "İstanbul 2. İcra Dairesi", "tur": "İcra Dosyası", "acik": True, "durum": "Açık",
                          "islem_durumu_etiket": "Açık", "acilis_tarihi": "2026-03-10", "uyap": {}, "talimat": False},
                "takip": {}, "taraflar": [], "tebligatlar": [], "alinamayan": []}
    elif uc == "gecmis":
        veri = []
    elif uc == "dosyalar_kimlikleri":
        TAHMINLER.append(("kimlik", q.get("mod")))
        veri = [1, 2, 3] if q.get("mod") != "takip" else []
    elif uc == "dosyalar_istek":
        TAHMINLER.append(("istek", q.get("mod")))
        hedef = 0 if q.get("mod") == "takip" else 3
        eksik = q.get("mod") == "eksik"
        veri = {"mod": q.get("mod"), "dosya": 7, "hedef": hedef, "takip": 1, "taraf_tum": 1, "taraf_kimliksiz": 2, "sayfa": 3,
                "istek": (5 if eksik else 9) if hedef else 0, "sure_sn": 27, "en_az": q.get("mod") in ("liste", "hepsi"), "acik_toplam": 1200,
                "liste_zamani": {"zaman": LISTE_ZAMANI, "alinan": 1188, "yaklasik": False}}
    elif uc == "kesinlestirme":
        KSORGULAR.append(q)
        satirlar = [ks_satir(i) for i in range(1, 6)]
        veri = {"toplam": len(satirlar), "sayfa": 1, "boyut": 100, "satirlar": satirlar, "secenek_siniri": 300, "genel_toplam": 812,
                "rol_bilinmiyor": 37, "avukat_bilinmiyor": False, "bugun": "2026-11-22",
                "gruplar": [{"kod": k, "etiket": e} for k, e in KS_GRUPLAR],
                "secenekler": {"grup": secenek("grup", [("kesinlestirilebilir", 2), ("sure_isliyor", 1), ("bekleniyor", 1), ("uyap_kesinlesti", 1)], dict(KS_GRUPLAR).get),
                               "birim": [{"deger": b, "etiket": b, "adet": 1} for b in DAIRELER[:5]], "dosya_no": [], "alacakli": [], "borclu": [],
                               "takip_yolu": [], "asama": secenek("asama", [("teblig", 3), ("kesinlesti", 2)]),
                               "mazbata": secenek("mazbata", [("dondu", 3), ("donmedi", 2)]), "uyap": secenek("uyap", [("", 4), ("evet", 1)]),
                               "talep": secenek("talep", [("", 5)]), "yeniden": secenek("yeniden", [("", 4), ("uygun", 1)]),
                               "yil": secenek("yil", [("2026", 5)]),
                               "acilis_yil": secenek("acilis_yil", [("2026", 5)]), "acilis_ay": [], "kesinlesme_ay": []}}
    elif uc == "kesinlestirme_istek":
        TAHMINLER.append(("ks_istek", q.get("is")))
        veri = {"is": q.get("is"), "satir": 5, "dosya": 2, "borclu": 2, "istek": 10, "sure_sn": 30, "tutar": 530.0, "uygun": 1, "farkli": 1}
    elif uc == "kesinlestirme_kimlikleri":
        TAHMINLER.append(("ks_kimlik", q.get("is")))
        veri = {"dosyalar": [11, 12], "borclular": [101, 102]}
    elif uc == "ozet":
        veri = {"borclu": 0, "acik_icra_dosya": 0, "icra_dosya": 7, "asamalar": [], "birimler": []}
    else:
        veri = None
    route.fulfill(status=200, content_type="application/json", body=json.dumps({"ok": True, "veri": veri}))

def gt_karsila(route):
    if route.request.method == "POST":
        POSTLAR.append(json.loads(route.request.post_data or "{}"))
        t = {"id": 1, "durum": "suruyor", "mod": POSTLAR[-1]["mod"], "asama": "taraf", "i": 0, "toplam": 3, "alinan": 0, "yeni_dosya": 0}
    else:
        t = {"id": 1, "durum": "bitti", "mod": POSTLAR[-1]["mod"] if POSTLAR else "liste", "mesaj": "Tamamlandı."}
    route.fulfill(status=200, content_type="application/json", body=json.dumps({"tarama": t}))

SAYFA = """<!doctype html><html lang=tr data-cu-tema=armonia><head><meta charset=utf-8></head>
<body style="margin:0"><div id=av-sidebar-wrapper style="position:fixed;left:0;top:60px;width:220px;bottom:0"><div class=sidebar-top-part></div></div>
<div id=page-content-wrapper style="margin-left:220px;height:100vh"></div></body></html>"""

with sync_playwright() as p:
    b = p.chromium.launch()
    s = b.new_page(viewport={"width": 1500, "height": 900}, locale="tr-TR")
    hatalar = []
    s.on("pageerror", lambda x: hatalar.append(str(x)))
    s.route("**/__cu__/veri/**", karsila)
    s.route("**/__uyap_agent__/icra_guncelle**", gt_karsila)
    s.route("http://sahte.test/", lambda r: r.fulfill(status=200, content_type="text/html", body=SAYFA))
    s.goto("http://sahte.test/")
    if TEMA:
        s.add_style_tag(path=TEMA)
    s.add_script_tag(path=JS)
    s.evaluate("window.__cuIcraTakipAc()")
    s.wait_for_selector("#cu-icra table.dl tbody tr")

    # Varsayılan sekme Dosya Listesi, açık dosyalar; Kapanış sütunu yok
    assert s.inner_text("#cu-icra .i-sekmeler button.on") == "Dosya Listesi"
    assert SORGULAR[-1].get("durum") == "acik", SORGULAR[-1]
    basliklar = s.eval_on_selector_all("#cu-icra table.dl th", "x => x.map(t => t.innerText.trim())")
    assert basliklar[:6] == ["İcra Dairesi", "Dosya No", "Dosya Türü", "Alacaklı", "Borçlu", "Rolüm"], basliklar
    assert "Kapanış" not in basliklar
    # Dosya türü sütunu: MTS'den gelen icra dosyası ayrı yazılır
    assert "İcra Dosyası (MTS'den)" in s.inner_text("#cu-icra table.dl")
    # Birden çok alacaklı/borçlu: ilki + "+N"
    assert s.locator("#cu-icra table.dl .fazla").count() > 0
    # Rolüm: tür + vekili olunan taraf
    assert s.locator("#cu-icra table.dl td.rol .rol-taraf").count() > 0
    assert "AHMET YILMAZ" in s.inner_text("#cu-icra table.dl td.rol")
    s.screenshot(path=os.path.join(CIKTI, "dl1_liste.png"))

    # İcra dairesi penceresi: alfabetik (Türkçe), Tümünü seç + gruplar
    s.click("#cu-icra table.dl th button[data-dl=birim]")
    s.wait_for_selector("#cu-icra .i-dlp")
    adlar = s.eval_on_selector_all("#cu-icra .i-dlp .liste label:not(.grup) .ad", "x => x.map(t => t.innerText)")
    assert adlar == ["Ankara 3. İcra Dairesi", "Ankara Banka Alacakları İcra Dairesi", "Cizre 1. İcra Dairesi", "Çorum 1. İcra Dairesi",
                     "İstanbul 2. İcra Dairesi", "İstanbul 11. İcra Dairesi", "İstanbul Abonelik Sözleşmeleri İcra Dairesi"], adlar
    gruplar = s.eval_on_selector_all("#cu-icra .i-dlp .liste label.grup .ad", "x => x.map(t => t.innerText)")
    assert gruplar == ["Tümünü seç", "Banka", "Abonelik", "Merkezi Takip (MTS)", "Gayrimenkul Satış"], gruplar
    assert s.evaluate("document.activeElement === document.querySelector('#cu-icra .i-dlp input[type=text]')"), "metin kutusu odakta"
    s.screenshot(path=os.path.join(CIKTI, "dl2_daire_penceresi.png"))

    # Tümünü seç → Banka'yı çıkar: süzgeç "!" + banka daireleri
    s.click("#cu-icra .i-dlp label.grup:has-text('Tümünü seç') input")
    s.wait_for_function("() => { const k = document.querySelector('#cu-icra .i-dlp label.grup input'); return k && k.checked; }")
    assert SORGULAR[-1]["birim"] == "!", SORGULAR[-1]
    s.click("#cu-icra .i-dlp label.grup:has-text('Banka') input")
    s.wait_for_function("() => document.querySelector('#cu-icra .i-dlp') !== null")
    s.wait_for_timeout(200)
    assert SORGULAR[-1]["birim"] == "!Ankara Banka Alacakları İcra Dairesi", SORGULAR[-1]
    assert not s.is_checked("#cu-icra .i-dlp label.grup:has-text('Banka') input")
    assert s.locator("#cu-icra table.dl th button[data-dl=birim].dolu").count() == 1, "süzgeç işareti"
    s.screenshot(path=os.path.join(CIKTI, "dl3_banka_haric.png"))
    # Temizle
    s.click("#cu-icra .i-dlp .alt-cubuk .i-dahasi")
    s.wait_for_timeout(200)
    assert "birim" not in SORGULAR[-1], SORGULAR[-1]

    # Yazarak süzme: tablo süzülür (bekleme sonrası), odak korunur
    if not s.locator("#cu-icra .i-dlp").count():
        s.click("#cu-icra table.dl th button[data-dl=birim]")
    s.fill("#cu-icra .i-dlp input[type=text]", "istanbul")
    s.wait_for_timeout(600)
    assert SORGULAR[-1].get("birim") == "istanbul", SORGULAR[-1]
    assert s.evaluate("document.activeElement === document.querySelector('#cu-icra .i-dlp input[type=text]')"), "yeniden çizimde odak korunur"
    gorunen = s.eval_on_selector_all("#cu-icra .i-dlp .liste label .ad", "x => x.map(t => t.innerText)")
    assert all("İstanbul" in g for g in gorunen) and len(gorunen) == 3, gorunen
    # Sıralama Z → A
    s.click("#cu-icra .i-dlp .sirala button:has-text('Z → A')")
    s.wait_for_timeout(200)
    assert (SORGULAR[-1].get("sirala"), SORGULAR[-1].get("yon")) == ("birim", "azalan"), SORGULAR[-1]
    # Dışarı tıklayınca kapanır
    s.click("#cu-icra .i-dl-ipucu span")
    s.wait_for_timeout(100)
    assert s.locator("#cu-icra .i-dlp").count() == 0

    # Rolüm: kodlu, metin kutusu yok
    s.click("#cu-icra table.dl th button[data-dl=rol]")
    assert s.locator("#cu-icra .i-dlp input[type=text]").count() == 0
    s.click("#cu-icra .i-dlp label:has-text('Borçlu vekili') input")
    s.wait_for_timeout(200)
    assert SORGULAR[-1].get("rol") == "borclu", SORGULAR[-1]
    s.keyboard.press("Escape")
    s.click("#cu-icra .i-dl-ipucu span")

    # Kapalı: Kapanış sütunu ve kapanış tarihi aralığı
    s.click("#cu-icra .i-seg button:has-text('Kapalı')")
    s.wait_for_selector("#cu-icra input[aria-label='Kapanış tarihi: başlangıç']")
    assert SORGULAR[-1].get("durum") == "kapali"
    s.fill("#cu-icra input[aria-label='Kapanış tarihi: başlangıç']", "2026-03-01")
    s.wait_for_timeout(300)
    assert SORGULAR[-1].get("kapanis_bas") == "2026-03-01", SORGULAR[-1]
    basliklar = s.eval_on_selector_all("#cu-icra table.dl th", "x => x.map(t => t.innerText.trim())")
    assert basliklar[-1] == "Kapanış", basliklar
    assert "12.03.2026" in s.inner_text("#cu-icra table.dl tbody")
    s.screenshot(path=os.path.join(CIKTI, "dl4_kapali.png"))
    # Açık'a dönünce kapanış aralığı gönderilmez
    s.click("#cu-icra .i-seg button:has-text('Açık')")
    s.wait_for_timeout(300)
    assert "kapanis_bas" not in SORGULAR[-1], SORGULAR[-1]

    # Dosya sayısı belirgin: listelenen + veritabanındaki toplam
    assert "7 dosya listeleniyor" in s.inner_text("#cu-icra .i-dl-sayi") and "5.312" in s.inner_text("#cu-icra .i-dl-sayi")
    dugmeler = s.eval_on_selector_all("#cu-icra .i-arac > button", "x => x.map(t => t.innerText)")
    assert dugmeler == ["Tüm verileri güncelle", "Dosya listesini güncelle", "Takip türlerini güncelle", "Tarafları güncelle"], dugmeler

    # Takip türleri: eksik yok → istek gitmez, yalnız "Kapat"
    s.click("#cu-icra .i-arac button:has-text('Takip türlerini güncelle')")
    s.wait_for_selector("#cu-icra .i-gt[aria-label='Takip türlerini güncelle']")
    assert "hiçbirinde eksik yok" in s.inner_text("#cu-icra .i-gt")
    assert s.locator("#cu-icra .i-gt button:has-text('Başlat')").count() == 0
    s.click("#cu-icra .i-gt button:has-text('Kapat')")
    assert not POSTLAR

    # Taraflar: önce tahmin (dosya ve istek sayısı), Başlat → yalnız eksiği olan dosyalar
    s.click("#cu-icra .i-arac button:has-text('Tarafları güncelle')")
    s.wait_for_selector("#cu-icra .i-gt[aria-label='Tarafları güncelle']")
    metin = s.inner_text("#cu-icra .i-gt")
    assert "7 dosyadan 3 dosyaya istek gidecek" in metin and "9 istek" in metin and "borçlu kimliği eksik: 2" in metin, metin
    assert not POSTLAR, "Başlat'a basmadan istek gitmez"
    s.screenshot(path=os.path.join(CIKTI, "dl5_tahmin.png"))
    s.click("#cu-icra .i-gt button:has-text('Başlat')")
    s.wait_for_timeout(300)
    assert POSTLAR and POSTLAR[-1] == {"mod": "taraf", "dosyalar": [1, 2, 3]}, POSTLAR
    assert ("kimlik", "taraf") in TAHMINLER, TAHMINLER
    s.wait_for_selector("#cu-icra .i-gt")
    s.screenshot(path=os.path.join(CIKTI, "dl6_guncelleme.png"))

    # Tüm veriler: dosya listesinin en son ne zaman alındığı söylenir, liste yeniden alınsın mı diye sorulur
    # (iki tahmin: listeyle "hepsi", listesiz "eksik"). Liste 2 saat önce alındı → "Hayır" öne çıkar.
    s.wait_for_function("() => !document.querySelector('#cu-icra .i-arac button').disabled")
    s.click("#cu-icra .i-arac button:has-text('Tüm verileri güncelle')")
    s.wait_for_selector("#cu-icra .i-gt[aria-label='Tüm verileri güncelle']")
    metin = s.inner_text("#cu-icra .i-gt[aria-label='Tüm verileri güncelle']")
    assert "Dosya listesi en son" in metin and "2 saat önce" in metin and "1.188 açık dosya" in metin, metin
    assert "yeniden alınsın mı" in metin and "en az 9 istek" in metin and "≈ 5 istek" in metin, metin
    assert ("istek", "hepsi") in TAHMINLER and ("istek", "eksik") in TAHMINLER, TAHMINLER
    assert s.locator("#cu-icra .i-gt button.ana:has-text('Hayır, yalnız eksikler')").count() == 1, "yakın zamanda alındı: Hayır öne çıkar"
    assert not POSTLAR[2:], "seçmeden istek gitmez"
    s.screenshot(path=os.path.join(CIKTI, "dl8_liste_zamani.png"))
    s.click("#cu-icra .i-gt button:has-text('Evet, listeyi de al')")
    s.wait_for_timeout(300)
    assert POSTLAR[-1] == {"mod": "hepsi", "hedefler": [1, 2, 3]}, POSTLAR
    # Hayır: liste yeniden alınmaz; süzgeçteki eksik dosyalar seçili dosya olarak (listedeki sırayla) gider
    s.wait_for_function("() => !document.querySelector('#cu-icra .i-arac button').disabled")
    s.click("#cu-icra .i-arac button:has-text('Tüm verileri güncelle')")
    s.wait_for_selector("#cu-icra .i-gt button:has-text('Hayır, yalnız eksikler')")
    s.click("#cu-icra .i-gt button:has-text('Hayır, yalnız eksikler')")
    s.wait_for_timeout(300)
    assert POSTLAR[-1] == {"mod": "hepsi", "dosyalar": [1, 2, 3]}, POSTLAR
    assert ("kimlik", "eksik") in TAHMINLER, TAHMINLER
    # Dosya listesi: süzgeç uygulanmaz, kimlik istenmez; en son ne zaman alındığı yazılır
    s.wait_for_function("() => !document.querySelector('#cu-icra .i-arac button').disabled")
    s.click("#cu-icra .i-arac button:has-text('Dosya listesini güncelle')")
    s.wait_for_selector("#cu-icra .i-gt[aria-label='Dosya listesini güncelle']")
    metin = s.inner_text("#cu-icra .i-gt[aria-label='Dosya listesini güncelle']")
    assert "süzgeç uygulanmaz" in metin and "Dosya listesi en son" in metin, metin
    s.click("#cu-icra .i-gt button:has-text('Başlat')")
    s.wait_for_timeout(300)
    assert POSTLAR[-1] == {"mod": "liste"}, POSTLAR
    # Diğer sekmeler yerinde
    sekmeler = s.eval_on_selector_all("#cu-icra .i-sekmeler button", "x => x.map(t => t.innerText)")
    assert sekmeler == ["Dosya Listesi", "Kesinleştirme", "Süreçler", "Tebligatlar", "Kesinleşme Süreleri", "Resmî Tatiller"], sekmeler

    # ── 21.11.2026 eklemeleri ──
    # Önceki süzgeçler temizlenir
    s.wait_for_function("() => !document.querySelector('#cu-icra .i-arac button').disabled")
    if s.locator("#cu-icra .i-dl-suzgec-sag .i-dahasi").count():
        s.click("#cu-icra .i-dl-suzgec-sag .i-dahasi")
        s.wait_for_timeout(300)
    assert not any(k in SORGULAR[-1] for k in ("birim", "rol", "yil", "sira")), SORGULAR[-1]

    # Dosya No: yıl süzgeci (çoklu seçim), yeniden eskiye
    s.click("#cu-icra table.dl th button[data-dl=dosya_no]")
    s.wait_for_selector("#cu-icra .i-dlp .izgara")
    yillar = s.eval_on_selector_all("#cu-icra .i-dlp .izgara button span", "x => x.map(t => t.innerText)")
    assert yillar == ["2026", "2025", "2024"], yillar
    s.click("#cu-icra .i-dlp .izgara button:has-text('2025')")
    s.wait_for_timeout(250)
    assert SORGULAR[-1].get("yil") == "2025", SORGULAR[-1]
    s.click("#cu-icra .i-dlp .izgara button:has-text('2024')")
    s.wait_for_timeout(250)
    assert set(SORGULAR[-1].get("yil", "").split("\n")) == {"2025", "2024"}, SORGULAR[-1]
    assert s.locator("#cu-icra .i-dlp .izgara button.on").count() == 2
    assert s.locator("#cu-icra table.dl th button[data-dl=dosya_no].dolu").count() == 1
    s.screenshot(path=os.path.join(CIKTI, "dl9_yil.png"))
    s.keyboard.press("Escape")
    s.click("#cu-icra .i-dl-ipucu span")

    # Açılış yılı / ayı: yıl sonra ay; yalnız ay da olur
    s.click("#cu-icra [data-dl=acilis_ya]")
    s.wait_for_selector("#cu-icra .i-dlp[aria-label='Açılış yılı ve ayı']")
    aylar = s.eval_on_selector_all("#cu-icra .i-dlp .bolum:nth-of-type(2) .izgara button span", "x => x.map(t => t.innerText)")
    assert aylar[:3] == ["Ocak", "Şubat", "Mart"] and len(aylar) == 12, aylar
    s.click("#cu-icra .i-dlp .izgara button:has-text('Mart')")
    s.wait_for_timeout(250)
    assert SORGULAR[-1].get("acilis_ay") == "3" and "acilis_yil" not in SORGULAR[-1], SORGULAR[-1]
    s.click("#cu-icra .i-dlp .bolum:nth-of-type(1) .izgara button:has-text('2026')")
    s.wait_for_timeout(250)
    assert (SORGULAR[-1].get("acilis_yil"), SORGULAR[-1].get("acilis_ay")) == ("2026", "3"), SORGULAR[-1]
    assert "2026" in s.inner_text("#cu-icra [data-dl=acilis_ya]") and "Mart" in s.inner_text("#cu-icra [data-dl=acilis_ya]")
    s.screenshot(path=os.path.join(CIKTI, "dl10_yil_ay.png"))
    s.click("#cu-icra .i-dlp .alt-cubuk .ana")
    assert s.locator("#cu-icra .i-dlp").count() == 0

    # Süzgeç sırası: konuldukları sırayla gönderilir
    assert SORGULAR[-1].get("sira") == "yil,acilis_yil,acilis_ay" or SORGULAR[-1].get("sira") == "yil,acilis_ay,acilis_yil", SORGULAR[-1]
    ilk_sira = SORGULAR[-1]["sira"]
    assert ilk_sira == "yil,acilis_ay,acilis_yil", "ay önce konuldu: " + ilk_sira
    # Konan süzgeçler etiket olarak (Süreçler'deki gibi), ✕ ile kaldırılabilir
    etiketler = s.eval_on_selector_all("#cu-icra .i-dl-secili .et-ad", "x => x.map(t => t.innerText.replace(/\\s+/g, ' '))")
    assert len(etiketler) == 3 and etiketler[0].startswith("Dosya yılı"), etiketler
    # Süzgeçler penceresi: sırayla satırlar; ↑ ile yer değişir
    assert "Süzgeçler (3)" in s.inner_text("#cu-icra [data-dl=suzgecler]")
    s.click("#cu-icra [data-dl=suzgecler]")
    s.wait_for_selector("#cu-icra .i-dlp[aria-label='Süzgeçler'] .sira .sat")
    satirlar = s.eval_on_selector_all("#cu-icra .i-dlp .sira .sat .ad", "x => x.map(t => t.innerText)")
    assert [x.split(":")[0] for x in satirlar] == ["Dosya yılı", "Açılış ayı", "Açılış yılı"], satirlar
    s.screenshot(path=os.path.join(CIKTI, "dl11_suzgecler.png"))
    s.click("#cu-icra .i-dlp .sira .sat:nth-child(3) button[aria-label^='Açılış yılı süzgecini yukarı']")
    s.wait_for_timeout(300)
    assert SORGULAR[-1].get("sira") == "yil,acilis_yil,acilis_ay", SORGULAR[-1]
    assert s.locator("#cu-icra .i-dlp[aria-label='Süzgeçler']").count() == 1, "pencere açık kalır"
    # Sürükle bırak: Açılış ayı en üste
    s.drag_and_drop("#cu-icra .i-dlp .sira .sat[data-k=acilis_ay]", "#cu-icra .i-dlp .sira .sat[data-k=yil]")
    s.wait_for_timeout(300)
    assert SORGULAR[-1].get("sira") == "acilis_ay,yil,acilis_yil", SORGULAR[-1]
    # Pencereden ✕: süzgeç kalkar, sıradan çıkar
    s.click("#cu-icra .i-dlp .sira .sat[data-k=yil] .x")
    s.wait_for_timeout(300)
    assert "yil" not in SORGULAR[-1] and SORGULAR[-1].get("sira") == "acilis_ay,acilis_yil", SORGULAR[-1]
    s.keyboard.press("Escape")
    s.click("#cu-icra .i-dl-ipucu span")
    # Etiketteki ✕
    s.locator("#cu-icra .i-dl-secili .et").first.locator(".et-x").click()
    s.wait_for_timeout(300)
    assert "acilis_ay" not in SORGULAR[-1] and "sira" not in SORGULAR[-1] and SORGULAR[-1].get("acilis_yil") == "2026", SORGULAR[-1]

    # Satıra tıklayınca dosya paneli açılır, liste en üste ÇIKMAZ
    s.click("#cu-icra .i-dl-suzgec-sag .i-dahasi")
    COK["satir"] = 80
    s.click("#cu-icra .i-seg button:has-text('Tümü')")
    s.wait_for_function("() => document.querySelectorAll('#cu-icra table.dl tbody').length >= 80")
    s.evaluate("document.querySelector('#cu-icra .i-ana').scrollTop = 1500")
    s.wait_for_timeout(100)
    once = s.evaluate("document.querySelector('#cu-icra .i-ana').scrollTop")
    assert once > 1000, once
    # Kullanıcının gördüğü satıra tıklama (Playwright'ın kendi kaydırması olmadan)
    s.evaluate("document.querySelectorAll('#cu-icra table.dl tbody')[39].querySelector('td:nth-child(2)').click()")
    s.wait_for_selector("#cu-icra .i-panel")
    s.wait_for_timeout(200)
    sonra = s.evaluate("document.querySelector('#cu-icra .i-ana').scrollTop")
    assert abs(sonra - once) < 5, (once, sonra)
    assert s.locator("#cu-icra table.dl tbody.sec").count() == 1
    s.screenshot(path=os.path.join(CIKTI, "dl12_panel_kaydirma.png"))
    # Paneli kapatınca da yer korunur
    s.click("#cu-icra .i-panel button[aria-label='Paneli kapat']")
    s.wait_for_timeout(200)
    assert abs(s.evaluate("document.querySelector('#cu-icra .i-ana').scrollTop") - once) < 5
    # Süzgeç değişince baştan başlar
    s.click("#cu-icra .i-seg button:has-text('Açık')")
    s.wait_for_timeout(300)
    assert s.evaluate("document.querySelector('#cu-icra .i-ana').scrollTop") == 0
    # Kapalı: kapanış yıl / ay düğmesi
    s.click("#cu-icra .i-seg button:has-text('Kapalı')")
    s.wait_for_selector("#cu-icra [data-dl=kapanis_ya]")
    s.click("#cu-icra [data-dl=kapanis_ya]")
    s.click("#cu-icra .i-dlp .izgara button:has-text('Mart')")
    s.wait_for_timeout(250)
    assert SORGULAR[-1].get("kapanis_ay") == "3", SORGULAR[-1]
    s.click("#cu-icra .i-seg button:has-text('Açık')")
    s.wait_for_timeout(250)
    assert "kapanis_ay" not in SORGULAR[-1], "kapanış süzgeci açık dosyalarda gönderilmez"
    COK["satir"] = 0

    # ── Kesinleştirme sekmesi (22.11.2026) ──
    s.wait_for_function("() => !document.querySelector('#cu-icra .i-arac button').disabled")
    s.click("#cu-icra .i-sekmeler button:has-text('Kesinleştirme')")
    s.wait_for_selector("#cu-icra .i-asamalar .i-asama")
    s.wait_for_function("() => document.querySelectorAll('#cu-icra table.dl tbody').length === 5")
    assert KSORGULAR and "durum" not in KSORGULAR[-1], KSORGULAR[-1:]
    basliklar = s.eval_on_selector_all("#cu-icra table.dl th", "x => x.map(t => t.innerText.trim())")
    assert basliklar == ["İcra Dairesi", "Dosya No", "Alacaklı", "Borçlu", "Takip Yolu", "Aşama", "Ödeme Emri Tebligatı", "Kesinleşme",
                         "Durum", "UYAP", "Talep", "Yeniden Tebliğ", "Açılış"], basliklar
    gruplar = s.eval_on_selector_all("#cu-icra .i-asamalar .i-asama", "x => x.map(t => t.innerText.replace(/\\s+/g, ' ').trim())")
    assert gruplar[0] == "2 Kesinleştirilebilir" and len(gruplar) == 7 and gruplar[1].startswith("0 "), gruplar
    metin = s.inner_text("#cu-icra")
    assert "5 borçlu listeleniyor" in metin and "812" in metin and "Rolü bilinmeyen 37 borçlu" in metin, metin[:600]
    assert "3 gün kaldı" in metin and "görüldü 21.11.2026" in metin, "kalan gün ve UYAP günü"
    dugmeler = s.eval_on_selector_all("#cu-icra .i-arac > button", "x => x.map(t => t.innerText)")
    assert dugmeler == ["Kesinleştirme talebi gönder", "Kesinleşmeyi UYAP'tan oku", "Mernis adresini sorgula", "Yeniden tebliğ talebi gönder"], dugmeler
    assert "21/2 şerhli tebliğe uygun" in metin, "Yeniden Tebliğ sütunu"
    s.screenshot(path=os.path.join(CIKTI, "ks1_liste.png"))
    # Grup düğmesi süzer
    s.click("#cu-icra .i-asamalar .i-asama:first-child")
    s.wait_for_timeout(300)
    assert KSORGULAR[-1].get("grup") == "kesinlestirilebilir", KSORGULAR[-1]
    assert s.locator("#cu-icra .i-asamalar .i-asama.on").count() == 1
    assert s.locator("#cu-icra table.dl th button[data-dl=grup].dolu").count() == 1
    # Ortak süzgeç penceresi: Aşama (kodlu)
    s.click("#cu-icra table.dl th button[data-dl=asama]")
    s.wait_for_selector("#cu-icra .i-dlp")
    assert s.locator("#cu-icra .i-dlp input[type=text]").count() == 0
    s.click("#cu-icra .i-dlp label:has-text('teblig') input")
    s.wait_for_timeout(250)
    assert KSORGULAR[-1].get("asama") == "teblig" and KSORGULAR[-1].get("sira") == "grup,asama", KSORGULAR[-1]
    s.keyboard.press("Escape")
    s.click("#cu-icra .i-dl-ipucu span")
    # Kesinleşme sütunu: yalnız sıralama
    s.click("#cu-icra table.dl th button[data-dl=kesinlesme]")
    s.wait_for_selector("#cu-icra .i-dlp")
    assert "kesinleşme gününe göre" in s.inner_text("#cu-icra .i-dlp") and s.locator("#cu-icra .i-dlp button:has-text('Yıl / ay')").count() == 0
    s.click("#cu-icra .i-dlp .sirala button:has-text('Yeniden eskiye')")
    s.wait_for_timeout(250)
    assert (KSORGULAR[-1].get("sirala"), KSORGULAR[-1].get("yon")) == ("kesinlesme", "azalan"), KSORGULAR[-1]
    s.click("#cu-icra .i-dl-ipucu span")
    # Talep: önce onay (geri alınamaz uyarısı), sonra seçili borçlular ve dosyaları gider
    onceki = len(POSTLAR)
    s.click("#cu-icra .i-arac button:has-text('Kesinleştirme talebi gönder')")
    s.wait_for_selector("#cu-icra .i-gt[aria-label='Kesinleştirme talebi gönder']")
    metin = s.inner_text("#cu-icra .i-gt[aria-label='Kesinleştirme talebi gönder']")
    assert "2 borçluya (2 dosya)" in metin and "geri alınamaz" in metin and "zaten kesinleşmiş" in metin, metin
    assert len(POSTLAR) == onceki, "onaysız gönderilmez"
    s.screenshot(path=os.path.join(CIKTI, "ks2_talep_onay.png"))
    s.click("#cu-icra .i-gt button:has-text('E-imzala ve gönder (2 borçlu)')")
    s.wait_for_timeout(300)
    assert POSTLAR[-1] == {"mod": "kesinlestirme_talebi", "dosyalar": [11, 12], "borclular": [101, 102]}, POSTLAR[-1]
    assert ("ks_istek", "talep") in TAHMINLER and ("ks_kimlik", "talep") in TAHMINLER, TAHMINLER
    # Kesinleşmeyi oku: yalnız dosyalar
    s.wait_for_function("() => !document.querySelector('#cu-icra .i-arac button').disabled")
    s.click("#cu-icra .i-arac button:has-text('Kesinleşmeyi UYAP')")
    s.wait_for_selector("#cu-icra .i-gt button:has-text('Başlat')")
    assert "sorgu yapılmaz" in s.inner_text("#cu-icra .i-gt")
    s.click("#cu-icra .i-gt button:has-text('Başlat')")
    s.wait_for_timeout(300)
    assert POSTLAR[-1] == {"mod": "kesinlesme", "dosyalar": [11, 12]}, POSTLAR[-1]
    # Mernis sorgusu: salt okuma, borçlularla gider
    s.wait_for_function("() => !document.querySelector('#cu-icra .i-arac button').disabled")
    s.click("#cu-icra .i-arac button:has-text('Mernis adresini sorgula')")
    s.wait_for_selector("#cu-icra .i-gt[aria-label='Mernis adresini sorgula'] button:has-text('Başlat')")
    assert "ilk ödeme emri zarfının adresiyle" in s.inner_text("#cu-icra .i-gt[aria-label='Mernis adresini sorgula']")
    s.click("#cu-icra .i-gt[aria-label='Mernis adresini sorgula'] button:has-text('Başlat')")
    s.wait_for_timeout(300)
    assert POSTLAR[-1] == {"mod": "mernis", "dosyalar": [11, 12], "borclular": [101, 102]}, POSTLAR[-1]
    # Yeniden tebliğ: ücret uyarısı + "anladım" kutusu işaretlenmeden gönderilmez
    s.wait_for_function("() => !document.querySelector('#cu-icra .i-arac button').disabled")
    onceki = len(POSTLAR)
    s.click("#cu-icra .i-arac button:has-text('Yeniden tebliğ talebi gönder')")
    kutu = "#cu-icra .i-gt[aria-label='Yeniden tebliğ talebi gönder']"
    s.wait_for_selector(kutu)
    metin = s.inner_text(kutu)
    assert "ÜCRETLİDİR" in metin and "530 TL" in metin and "1 borçluya T.K. 21/2 şerhli, 1 borçluya Mernis adresine normal tebligat" in metin, metin
    assert s.locator(kutu + " button:has-text('E-imzala ve gönder')").is_disabled(), "anladım işaretlenmeden kapalı"
    s.screenshot(path=os.path.join(CIKTI, "ks4_yeniden_teblig_onay.png"))
    s.click(kutu + " input[type=checkbox]")
    s.wait_for_timeout(150)
    assert len(POSTLAR) == onceki, "onaysız gönderilmez"
    s.click(kutu + " button:has-text('E-imzala ve gönder')")
    s.wait_for_timeout(300)
    assert POSTLAR[-1] == {"mod": "yeniden_teblig", "dosyalar": [11, 12], "borclular": [101, 102]}, POSTLAR[-1]
    assert ("ks_istek", "teblig") in TAHMINLER and ("ks_kimlik", "mernis") in TAHMINLER, TAHMINLER
    # Satıra tıklayınca dosya paneli
    s.wait_for_function("() => !document.querySelector('#cu-icra .i-arac button').disabled")
    s.click("#cu-icra table.dl tbody:nth-of-type(1) td:nth-child(2)")
    s.wait_for_selector("#cu-icra .i-panel")
    s.screenshot(path=os.path.join(CIKTI, "ks3_panel.png"))
    s.click("#cu-icra .i-panel button[aria-label='Paneli kapat']")
    # Dosya Listesi'ne dönünce kendi süzgeçleri: kesinleştirme süzgeçleri ona karışmaz; geri gelince korunur
    s.click("#cu-icra .i-sekmeler button:has-text('Dosya Listesi')")
    s.wait_for_selector("#cu-icra .i-seg")
    s.wait_for_timeout(300)
    assert "grup" not in SORGULAR[-1] and "asama" not in SORGULAR[-1], SORGULAR[-1]
    s.click("#cu-icra .i-sekmeler button:has-text('Kesinleştirme')")
    s.wait_for_timeout(400)
    assert KSORGULAR[-1].get("grup") == "kesinlestirilebilir" and KSORGULAR[-1].get("asama") == "teblig", KSORGULAR[-1]

    # Koyu tema
    s.evaluate("document.documentElement.setAttribute('data-cu-tema', 'koyu')")
    s.screenshot(path=os.path.join(CIKTI, "dl7_koyu.png"))

    assert not hatalar, hatalar
    print("TAMAM:", len(SORGULAR), "sorgu,", len(POSTLAR), "güncelleme isteği")
    b.close()
