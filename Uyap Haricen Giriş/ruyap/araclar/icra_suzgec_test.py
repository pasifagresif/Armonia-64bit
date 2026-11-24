# İcra Takip Süreçleri: başlık süzgeçleri (künye + tebligat) + borçlunun tebligat satırları — sahte uçlarla tarayıcı denemesi.
import json, sys, os, re, urllib.parse
from playwright.sync_api import sync_playwright

JS = sys.argv[1]
CIKTI = sys.argv[2]

def tk(x):
    x = (x or "").replace("İ", "i").replace("I", "ı").lower()
    for a, b in zip("ışğüöç", "isguoc"):
        x = x.replace(a, b)
    return x

def teb(i, tur, ptt, mazbata, eteb=False):
    return {"id": i, "tur": tur, "evrak_tarihi": "0%d/10/2026" % (i % 9 + 1), "barkod": "" if eteb else "40076635%05d" % i,
            "elektronik": eteb, "ptt_durumu": ptt, "son_islem_tarihi": "05/10/2026 14:30" if ptt else "",
            "teslim_sekli": "Muhataba bizzat teslim" if ptt == "TESLİM EDİLDİ" else ("Tanınmıyor" if ptt == "İADE" else ""),
            "mazbata_var": mazbata, "mazbata": "Tebliğ mazbatası" if mazbata else ""}

KES = {"kesinlesti": "Kesinleşti", "kesinlesme_bekliyor": "Kesinleşme Bekliyor", "itiraz": "İtiraz", "": ""}

def teb_kes(t, borclu_kes):
    """Sunucudaki tebligat başına kesinleşme: ödeme emri borçlunun hesabı, 103 / kıymet takdiri 7 gün sabit, diğerlerinde yok."""
    if t["tur"] == "Ödeme Emri":
        return {"kesinlesme": KES[borclu_kes], "kesinlesme_yok": False}
    if t["tur"] in ("103 Davetiyesi", "Kıymet Takdiri Raporu"):
        return {"kesinlesme": "Kesinleşme Bekliyor (16.10.2026)" if t["ptt_durumu"] == "TESLİM EDİLDİ" else "Kesinleşme Bekliyor", "kesinlesme_yok": False}
    return {"kesinlesme": "", "kesinlesme_yok": True}
BORCLULAR = [
    (1, "2026/101", "AHMET YILMAZ", "X BANKASI A.Ş.", "kesinlesti", "kesinlesti",
     [teb(1, "Ödeme Emri", "TESLİM EDİLDİ", True), teb(2, "103 Davetiyesi", "İADE", False), teb(3, "Kıymet Takdiri Raporu", "", False),
      teb(4, "Satış İlanı", "TESLİM EDİLDİ", True), teb(5, "Ödeme Emri", "İADE", False)]),
    (2, "2026/102", "IŞIL ÇİÇEK", "X BANKASI A.Ş.", "teblig", "kesinlesme_bekliyor",
     [teb(6, "Ödeme Emri", "TESLİM EDİLDİ", False)]),
    (3, "2026/103", "MEHMET DEMİR", "Y FİNANS A.Ş.", "iade", "",
     [teb(7, "Ödeme Emri", "İADE", True)]),
    (4, "2026/104", "ZEYNEP KAYA", "Y FİNANS A.Ş.", "takip_talebi", "", []),
    (5, "2026/105", "ÖRNEK İNŞAAT LTD. ŞTİ.", "X BANKASI A.Ş.", "teblig", "itiraz",
     [teb(8, "Ödeme Emri", "E-Tebligat: tebliğ edildi (süre 5 gün sonra başlar)", False, True)]),
    (None, "2025/77", "", "Y FİNANS A.Ş.", "", "", []),
]
# bag → (daire, dosya türü, açılış, açık mı)
KUNYE = {1: ("İzmir 5. İcra Dairesi", "İcra Dosyası", "2026-01-10", True), 2: ("İzmir 5. İcra Dairesi", "İcra Dosyası", "2026-02-10", True),
         3: ("Ankara 3. İcra Dairesi", "MTS", "2026-03-10", True), 4: ("Ankara 3. İcra Dairesi", "MTS", "2026-04-10", True),
         5: ("İzmir 5. İcra Dairesi", "İcra Dosyası", "2026-05-10", True), None: ("Ankara 3. İcra Dairesi", "MTS", "2025-03-15", False)}
son_sorgu = {}

def haric(f):
    r = f[1:]
    return [] if r == "" else [""] if r == "\n" else r.split("\n")

def metin_uyar(f, deger):
    if f.startswith("!"):
        return not any(tk(x.strip()) == tk(deger.strip()) for x in haric(f))
    if f.startswith("="):
        return any(tk(x.strip()) == tk(deger.strip()) for x in f[1:].split("\n"))
    return not f.strip() or tk(f.strip()) in tk(deger)

def kodlu_uyar(f, uyar):
    if not f:
        return True
    if f.startswith("!"):
        return not any(uyar(x) for x in haric(f))
    return any(uyar(x) for x in f.split("\n"))

def liste(q):
    f = {k: q.get(k, [""])[0] for k in ("q", "asama", "tur", "teblig", "mazbata", "kesinlesme", "durum", "birim", "dosya_no", "alacakli", "borclu",
                                        "yil", "dosya_tur", "acilis_bas", "acilis_son")}
    tv, pv, mv = bool(f["tur"].strip()), bool(f["teblig"].strip()), bool(f["mazbata"])
    def tu(t, a, b, c):
        return ((not a or metin_uyar(f["tur"], t["tur"])) and (not b or metin_uyar(f["teblig"], t["ptt_durumu"]))
                and (not c or kodlu_uyar(f["mazbata"], lambda x: t["mazbata_var"] if x == "var" else not t["mazbata_var"])))
    say = {"tur": {}, "teblig": {}, "mazbata": {}, "kesinlesme": {}, "birim": {}, "dosya_no": {}, "alacakli": {}, "borclu": {}, "yil": {}, "dosya_tur": {}}
    satirlar = []
    for bag, no, ad, alacakli, asama, kes, tebs in BORCLULAR:
        birim, dtur, acilis, acik = KUNYE[bag]
        yil = no.split("/")[0]
        if f["asama"] and asama != f["asama"]:
            continue
        if (f["durum"] == "acik" and not acik) or (f["durum"] == "kapali" and acik):
            continue
        if (f["acilis_bas"] and acilis < f["acilis_bas"]) or (f["acilis_son"] and acilis > f["acilis_son"]):
            continue
        yazi = lambda s: s and not s.startswith("=") and not s.startswith("!")
        def ku_(dis):
            u = lambda k, v: dis != k or yazi(v)
            return ((not u("birim", f["birim"]) or metin_uyar(f["birim"], birim))
                    and (not u("dosya_no", f["dosya_no"]) or metin_uyar(f["dosya_no"], no))
                    and (not u("alacakli", f["alacakli"]) or metin_uyar(f["alacakli"], alacakli))
                    and (not u("borclu", f["borclu"]) or metin_uyar(f["borclu"], ad))
                    and (dis == "yil" or kodlu_uyar(f["yil"], lambda x: x == yil))
                    and (dis == "dosya_tur" or kodlu_uyar(f["dosya_tur"], lambda x: x == dtur)))
        for k, v in (("birim", birim), ("dosya_no", no), ("alacakli", alacakli), ("borclu", ad), ("yil", yil), ("dosya_tur", dtur)):
            if v and ku_(k):
                say[k][v] = say[k].get(v, 0) + 1
        if not ku_(None):
            continue
        if f["q"] and not all(k in tk(no + " " + ad + " " + alacakli) for k in tk(f["q"]).split()):
            continue
        ku = kodlu_uyar(f["kesinlesme"], lambda x: (x == "-" and not kes) or x == kes)
        if ku:
            for anahtar, deg in (("tur", {t["tur"] for t in tebs if tu(t, False, pv, mv)}),
                                 ("teblig", {t["ptt_durumu"] for t in tebs if tu(t, tv, False, mv)}),
                                 ("mazbata", {"var" if t["mazbata_var"] else "yok" for t in tebs if tu(t, tv, pv, False)})):
                for x in deg:
                    say[anahtar][x] = say[anahtar].get(x, 0) + 1
        tamam = not (tv or pv or mv) or any(tu(t, tv, pv, mv) for t in tebs)
        if tamam:
            say["kesinlesme"][kes or "-"] = say["kesinlesme"].get(kes or "-", 0) + 1
        if not (ku and tamam):
            continue
        satirlar.append({"bag": bag, "dosya": bag or 99, "birim": birim, "dosya_no": no, "dosya_durumu": "Açık" if acik else "Kapalı", "acik": acik,
                         "dosya_tur": dtur, "acilis_tarihi": acilis,
                         "borclu": ad, "alacakli": alacakli, "asama": asama, "asama_tarihi": "2026-10-05",
                         "kesinlesme": KES[kes], "kesinlesme_durumu": kes, "tebligat": "", "uyapta_gorulmedi": False,
                         "tebligatlar": [dict(t, uyan=(tv or pv or mv) and tu(t, True, True, True), **teb_kes(t, kes)) for t in tebs]})
    def sec(anahtar, etiket):
        return [{"deger": d, "etiket": etiket(d), "adet": n} for d, n in sorted(say[anahtar].items(), key=lambda x: (-x[1], x[0]))]
    return {"toplam": len(satirlar), "dosya_sayisi": len(satirlar), "sayfa": 1, "boyut": 50, "satirlar": satirlar, "secenek_siniri": 300, "secenekler": {
        "birim": sec("birim", str), "dosya_no": sec("dosya_no", str), "alacakli": sec("alacakli", str), "borclu": sec("borclu", str), "yil": sec("yil", str),
        "dosya_tur": sec("dosya_tur", str),
        "tur": sec("tur", lambda d: d or "(türü yok)"), "teblig": sec("teblig", lambda d: d or "Henüz sorgulanmadı"),
        "mazbata": sec("mazbata", lambda d: "Döndü" if d == "var" else "Dönmedi"),
        "kesinlesme": sec("kesinlesme", lambda d: "Henüz hesaplanmadı" if d == "-" else KES[d])}}

OZET = {"dosya": 5, "icra_dosya": 5, "acik_icra_dosya": 5, "tebligat": 8, "son_guncelleme": "2026-10-04 12:00", "borclu": 5,
        "birimler": [{"ad": "Ankara 3. İcra Dairesi", "birim_id": "2002", "acik": 2}, {"ad": "İzmir 5. İcra Dairesi", "birim_id": "5005", "acik": 3}],
        "asamalar": [{"kod": "takip_talebi", "etiket": "Takip Talebi", "adet": 1}, {"kod": "teblig", "etiket": "Tebliğ", "adet": 2},
                     {"kod": "iade", "etiket": "İade", "adet": 1}, {"kod": "kesinlesti", "etiket": "Kesinleşti", "adet": 1}]}
# Kesinleşme süreleri: takip türü ağacı (UYAP listesi + dosyalarda görülenler), kurallar, dağılım
HAZIR_VARSAYILAN = {"ad": "Varsayılan (tüm takipler)", "gun": 7, "itiraz_gun": 7, "itiraz_durdurur": True, "dayanak": ""}
KURALLAR = {"kurallar": [{"id": 1, "takibin_turu": "", "takibin_yolu": "", "takibin_sekli": "", "ad": "Varsayılan", "gun": 7, "aktif": True,
                          "aciklama": "", "turu_ad": "", "yolu_ad": "", "sekli_ad": "", "ornek_no": "", "itiraz_gun": 7, "itiraz_durdurur": True,
                          "dayanak": "", "elle_degisti": True, "adet": 40, "varsayilan": HAZIR_VARSAYILAN},
                         {"id": 2, "takibin_turu": "1", "takibin_yolu": "3", "takibin_sekli": "0", "ad": "İlamsız · Kambiyo, haciz yolu · Örnek 10",
                          "gun": 10, "aktif": True, "aciklama": "", "turu_ad": "İlamsız Takip", "yolu_ad": "Kambiyo Senetlerine Mahsus Haciz Yoluyla Takip",
                          "sekli_ad": "", "ornek_no": "", "itiraz_gun": 5, "itiraz_durdurur": False, "dayanak": "İİK 168–170", "elle_degisti": False,
                          "adet": 0, "varsayilan": {"ad": "İlamsız · Kambiyo, haciz yolu · Örnek 10", "gun": 10, "itiraz_gun": 5,
                                                     "itiraz_durdurur": False, "dayanak": "İİK 168–170"}}],
            "dagilim": [{"turu": "1", "yolu": "0", "sekli": "0", "adet": 12, "ornek": "İzmir 5. İcra Dairesi 2026/101", "turu_ad": "İlamsız Takip",
                         "yolu_ad": "Genel Haciz Yoluyla Takip", "sekli_ad": "ÖRNEK: 7 İlamsız Takiplerde Ödeme Emri - Eski No: 49", "ornek_no": "7"}],
            "takip_turu_bilinmeyen": 40, "liste_zamani": None,
            "agac": [{"kod": "0", "ad": "İlamlı Takip", "adet": 0, "yollar": [{"kod": "1", "ad": "İlamların İcrası", "adet": 0, "sekiller": [
                         {"kod": "0", "ad": "ÖRNEK: 4-5 Para Borcuna Dair İcra Emri - Eski No: 53", "ornek_no": "4-5", "adet": 0}]}]},
                     {"kod": "1", "ad": "İlamsız Takip", "adet": 12, "yollar": [
                         {"kod": "0", "ad": "Genel Haciz Yoluyla Takip", "adet": 12, "sekiller": [
                             {"kod": "0", "ad": "ÖRNEK: 7 İlamsız Takiplerde Ödeme Emri - Eski No: 49", "ornek_no": "7", "adet": 12},
                             {"kod": "1", "ad": "ÖRNEK: 10 Kambiyo Senetlerine Mahsus Haciz Yoluyla Takiplerde Ödeme Emri", "ornek_no": "10", "adet": 0}]},
                         {"kod": "2", "ad": "Kambiyo Senetlerine Mahsus Haciz Yoluyla Takip", "adet": 0, "sekiller": []}]}]}
KURAL_POST = []
KURAL_GET = []
# Tebligatlar sekmesi: tebligat başına satır (borçluya bağlanmamış banka zarfı ve talimat dosyası dahil)
def tsat(i, dosya, birim, no, tur, borclu, muhatap, ptt="", teslim="", kes="", kes_kod="-", neden="", tdosya="İcra Dosyası", takip="Örnek 7"):
    return {"id": i, "dosya": dosya, "birim": birim, "dosya_no": no, "dosya_tur": tdosya, "dosya_durumu": "Açık", "acik": True,
            "acilis_tarihi": "2026-10-01", "takip": takip, "alacakli": "X BANKASI A.Ş.", "borclu": borclu, "muhatap": muhatap,
            "muhatap_etiket": {"borclu": "Borçlu", "ucuncu": "Üçüncü kişi (banka, işveren …)"}[muhatap], "barkod": "40076635%05d" % i,
            "tur": tur, "evrak_no": i, "evrak_tarihi": "0%d/10/2026" % i, "ptt_durumu": ptt, "son_islem_tarihi": "", "teslim_sekli": teslim,
            "mazbata_var": False, "mazbata": "", "yeniden": False, "elektronik": False, "kesinlesme": kes, "kesinlesme_kod": kes_kod,
            "kesinlesme_neden": neden, "talimat": tdosya == "Talimat Dosyası"}
TEB_SATIR = [
    tsat(1, 1, "İzmir 5. İcra Dairesi", "2026/101", "Ödeme Emri", "AHMET YILMAZ", "borclu", "İADE", "Adres yetersiz", "Kesinleşti", "kesinlesti"),
    tsat(2, 1, "İzmir 5. İcra Dairesi", "2026/101", "Banka (Haciz İhbarnamesi)", "ARACIKISIKURUM DENİZBANK A.Ş.", "ucuncu", kes_kod="yok",
         neden="Bu tebligat türünde kesinleşme yok"),
    tsat(3, 7, "İzmir 5. İcra Dairesi", "2026/7", "Kıymet Takdiri Raporu", "VELİ KARA", "borclu", "TESLİM EDİLDİ", kes_kod="talimat",
         neden="Talimat dosyasında kesinleşme yok", tdosya="Talimat Dosyası", takip="Talimat"),
]
GRUP = {"Ödeme Emri": "Ödeme / İcra Emri", "Banka (Haciz İhbarnamesi)": "Banka", "Kıymet Takdiri Raporu": "Kıymet Takdiri"}
for _t in TEB_SATIR:
    _t.update({"grup": GRUP[_t["tur"]], "haciz_turu": _t["tur"] == "Banka (Haciz İhbarnamesi)", "haciz_sirasi": None,
               "zarf_icerigi": "4-5 ÖRNEK İCRA EMRİ VE İLAM SURETİ" if _t["id"] == 1 else "", "muhatap_adi": _t["borclu"]})
HACIZ_POST = []
TEB_ISTEK = []
TUR_POST = []
EXCEL_ISTEK = []
def teb_liste(q):
    f = {k: v[0] for k, v in q.items()}
    satir = [t for t in TEB_SATIR if (not f.get("muhatap") or t["muhatap"] in f["muhatap"].split("\n"))
             and (not f.get("tur") or metin_uyar(f["tur"], t["tur"])) and (not f.get("grup") or t["grup"] in f["grup"].split("\n"))]
    say = lambda k, et=str: [{"deger": d, "etiket": et(d), "adet": n} for d, n in
                             sorted({x[k]: sum(1 for y in TEB_SATIR if y[k] == x[k]) for x in TEB_SATIR}.items())]
    sec = {k: [] for k in ("birim", "dosya_no", "dosya_tur", "takip", "alacakli", "borclu", "barkod", "teblig", "teslim", "mazbata", "yeniden", "elektronik")}
    sec.update({"muhatap": say("muhatap", lambda d: {"borclu": "Borçlu", "ucuncu": "Üçüncü kişi (banka, işveren …)"}[d]), "tur": say("tur"),
                "kesinlesme": say("kesinlesme_kod"), "grup": say("grup")})
    return {"toplam": len(satir), "dosya_sayisi": len({t["dosya"] for t in satir}), "sayfa": 1, "boyut": 100, "satirlar": satir,
            "secenekler": sec, "secenek_siniri": 300}
istekler = []
HESAP = []  # "Yeniden hesapla" gövdeleri
# UYAP'tan güncelleme sahte ucu: sıradaki GET yanıtları kuyruktan, POST'lar kaydedilir.
GT = {"sira": [], "son": None, "postlar": []}
def gt_tarama(**k):
    v = {"id": "t1", "durum": "calisiyor", "mesaj": "", "asama": "taraf", "kapsam": "Bütün açık dosyalar", "i": 0, "toplam": 0,
         "kalan_sn": None, "mola_kalan_sn": None, "alinan": 0, "yeni_dosya": 0, "taraf_cekilen": 0, "kapandi": 0,
         "uyari_sayisi": 0, "uyarilar": [], "rapor": [], "captcha": None,
         "ptt_sorgusu": True, "ptt_kapandi": False, "teb_dosya": 0, "pdf_indirilen": 0, "ptt_sorulan": 0, "ptt_hatasi": 0,
         "teb_yazilan": 0, "teb_yeni": 0, "teb_degisen": 0, "teb_atlanan": 0}
    v.update(k)
    return v

# Tek dosya güncellemesi (dosya paneli): POST /dosya başlatır, GET /dosya/<id> kuyruktan yanıt verir.
DI = {"sira": [], "son": None, "postlar": []}
DOSYA = {"kunye": {"id": 1, "birim": "İzmir 5. İcra Dairesi", "dosya_no": "2026/101", "durum": "Açık", "acik": True, "tur": "İcra Dosyası",
                   "acilis_tarihi": "2026-01-10", "islem_durumu": "aktif", "islem_durumu_etiket": "Aktif", "islem_durumu_notu": "",
                   "bekleme_bitis": None, "son_guncelleme": "2026-11-19 10:00", "uyap": {}},
         "takip": {"okundu": False, "kesinlesme_suresi_gun": 7},
         "taraflar": [],
         "tebligatlar": [dict(teb(1, "Ödeme Emri", "DAĞITIMDA", False), evrak_no=5, muhatap="BORÇLU AHMET YILMAZ", hareket_sayisi=0, mazbata_elle=False),
                         dict(teb(2, "103 Davetiyesi", "", False), evrak_no=6, muhatap="BORÇLU AHMET YILMAZ", hareket_sayisi=0, mazbata_elle=False)],
         "mazbatalar": [{"evrak_no": 9, "tur": "Kapalı Tebliğ Mazbatası", "aciklama": "", "evrak_tarihi": "20/10/2026", "elle_tebligat": None}]}
MZ_POST = []

def gt_karsila(route):
    u = urllib.parse.urlparse(route.request.url)
    yol = u.path.split("/__uyap_agent__/icra_guncelle")[1]
    if yol.startswith("/dosya"):
        if route.request.method == "POST" and yol == "/dosya":
            DI["postlar"].append(json.loads(route.request.post_data or "{}"))
            DI["son"] = gt_tarama(id="d1", mod="dosya", asama="dosya", mesaj="İzmir 5. İcra Dairesi 2026/101 UYAP'ta aranıyor…")
        elif route.request.method == "POST" and yol.endswith("/iptal"):
            DI["son"] = gt_tarama(id="d1", mod="dosya", durum="iptal", mesaj="Durduruldu.")
            DI["sira"] = []
        elif DI["sira"]:
            DI["son"] = DI["sira"].pop(0)
        route.fulfill(status=200, content_type="application/json", body=json.dumps({"tarama": DI["son"]}))
        return
    if route.request.method == "POST":
        govde = json.loads(route.request.post_data or "{}")
        GT["postlar"].append((yol, govde))
        if yol == "":
            GT["son"] = gt_tarama(mod=govde.get("mod", "liste"), kapsam=govde.get("birim_adi") or "Bütün açık dosyalar", asama="liste", mesaj="Listeleniyor…")
        elif yol.endswith("/iptal"):
            GT["son"] = gt_tarama(mod=GT["son"].get("mod", "liste"), durum="iptal", asama="taraf", mesaj="Durduruldu.", rapor=["⚠️ Tarama durduruldu: 4/10 dosya veritabanına yazıldı."])
            GT["sira"] = []
        elif yol.endswith("/captcha"):
            GT["son"] = dict(GT["son"], durum="calisiyor", captcha=None)
    elif GT["sira"]:
        GT["son"] = GT["sira"].pop(0)
    route.fulfill(status=201 if route.request.method == "POST" and yol == "" else 200, content_type="application/json",
                  body=json.dumps({"tarama": GT["son"]}))

def karsila(route):
    u = urllib.parse.urlparse(route.request.url)
    uc = u.path.split("/__cu__/veri/")[1]
    q = urllib.parse.parse_qs(u.query, keep_blank_values=True)
    if uc == "ozet":
        veri = OZET
    elif uc == "borclular":
        son_sorgu.clear(); son_sorgu.update({k: v[0] for k, v in q.items()})
        istekler.append({k: v[0] for k, v in q.items() if v[0] and k not in ("sayfa", "boyut", "durum")})
        veri = liste(q)
    elif uc == "dosya_kimlikleri":
        kim = []
        for s_ in liste(q)["satirlar"]:
            if s_["dosya"] not in kim:
                kim.append(s_["dosya"])
        istekler.pop()
        veri = kim
    elif uc == "hesapla":
        HESAP.append(json.loads(route.request.post_data or "{}"))
        veri = {"guncellenen": 0, "olay": 0}
    elif uc == "dosya":
        veri = DOSYA
    elif uc == "tebligatlar":
        TEB_ISTEK.append({k: v[0] for k, v in q.items() if v[0] and k not in ("sayfa", "boyut", "durum")})
        veri = teb_liste(q)
    elif uc == "haciz_sirasi":
        HACIZ_POST.append(json.loads(route.request.post_data or "{}"))
        veri = None
    elif uc == "tebligat_dosya_kimlikleri":
        veri = sorted({t["dosya"] for t in teb_liste(q)["satirlar"]})
    elif uc == "tebligatlar_excel":
        EXCEL_ISTEK.append({k: v[0] for k, v in q.items()})
        route.fulfill(status=200, body=b"PK\x03\x04sahte", headers={"Content-Type": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                                                                "Content-Disposition": 'attachment; filename="Tebligatlar_20261119_1600.xlsx"'})
        return
    elif uc == "tur_eslestirme":
        veri = {"gruplar": [{"anahtar": "tensip zapti", "ornek": "Tensip zaptı", "adet": 12, "cesit": 3, "kaynak": "Zarf", "ornek_dosya": "İzmir 5. İcra Dairesi 2026/101",
                             "yazilislar": [{"yazilis": "Tensip zaptı", "adet": 10}, {"yazilis": "TENSİP ZAPTI", "adet": 2}]},
                            {"anahtar": "ilani satis", "ornek": "Satış İlanı", "adet": 4, "cesit": 1, "kaynak": "Mazbata", "ornek_dosya": "Ankara 3. İcra Dairesi 2026/104",
                             "yazilislar": [{"yazilis": "Satış İlanı", "adet": 4}]}],
                "turler": ["Ödeme Emri", "103 Davetiyesi", "Kıymet Takdiri Raporu"]}
    elif uc == "tur_eslestir":
        TUR_POST.append(json.loads(route.request.post_data or "{}"))
        veri = {"eslesme": 2, "degisen": 16, "guncellenen": 1}
    elif uc == "kurallar":
        KURAL_GET.append(1)
        veri = KURALLAR
    elif uc == "kural":
        KURAL_POST.append(json.loads(route.request.post_data or "{}"))
        veri = {"id": 9, "guncellenen": 2}
    elif uc == "kural_varsayilan":
        KURAL_POST.append({"varsayilan": json.loads(route.request.post_data or "{}")})
        veri = {"guncellenen": 1}
    elif uc == "gecmis":
        veri = []
    elif uc == "mazbata_bagla":
        MZ_POST.append(json.loads(route.request.post_data or "{}"))
        veri = {"degisen": 1}
    else:
        veri = {}
    route.fulfill(status=200, content_type="application/json", body=json.dumps({"ok": True, "veri": veri}))

SAYFA = """<!doctype html><html><head><meta charset=utf-8><style>body{margin:0;font-family:Segoe UI}</style></head>
<body><div id=av-sidebar-wrapper style="position:fixed;left:0;top:60px;width:220px;bottom:0"><div class=sidebar-top-part></div></div>
<div id=page-content-wrapper style="margin-left:220px;height:100vh"></div></body></html>"""

hatalar = []
def bekle(sayfa, n=450):
    sayfa.wait_for_timeout(n)

with sync_playwright() as p:
    b = p.chromium.launch()
    s = b.new_page(viewport={"width": 1500, "height": 900})
    s.on("pageerror", lambda e: hatalar.append(str(e)))
    # Doğrulama betiği testte bilerek engellenir (challenges.cloudflare.com); onun yüklenememe kaydı hata sayılmaz.
    s.on("console", lambda m: hatalar.append(m.text) if m.type == "error" and "challenges.cloudflare.com" not in (m.location or {}).get("url", "") else None)
    s.route("**/__cu__/veri/**", karsila)
    s.route("**/__uyap_agent__/icra_guncelle**", gt_karsila)
    s.route("https://challenges.cloudflare.com/**", lambda r: r.abort())
    s.route("http://sahte.test/", lambda r: r.fulfill(status=200, content_type="text/html", body=SAYFA))
    s.goto("http://sahte.test/")
    s.add_script_tag(path=JS)
    s.evaluate("window.__cuIcraTakipAc()")
    # 20.11.2026: varsayılan sekme Dosya Listesi; bu deneme Süreçler sekmesini sınar.
    s.click("#cu-icra .i-sekmeler button:has-text('Süreçler')")
    s.wait_for_selector("#cu-icra table.liste tbody.tik")
    say = lambda: s.locator("#cu-icra tbody.tik").count()
    kutu = lambda k: s.locator('#cu-icra .i-kutu-s[data-k="%s"] input' % k)

    # 1) Borçlunun tebligatları: ilk 3 + "+2 tebligat daha"; açınca 5
    assert say() == 5, say()
    ahmet = s.locator("#cu-icra tbody.tik").first
    assert ahmet.locator("tr").count() == 4, ahmet.locator("tr").count()
    assert "+2 tebligat daha" in ahmet.inner_text()
    ahmet.locator(".i-dahasi").click(); bekle(s, 100)
    ahmet = s.locator("#cu-icra tbody.tik").first
    assert ahmet.locator("tr").count() == 6 and "Daha az göster" in ahmet.inner_text()
    assert s.locator("#cu-icra .i-panel").count() == 0, "+N düğmesi dosya panelini açmamalı"
    assert "tebligat kaydı yok" in s.locator("#cu-icra tbody.tik").nth(3).inner_text()
    # Kesinleşme tebligat başına: ödeme emri → borçlunun kesinleşmesi; 103 → kendi süresi; satış ilanı → yok
    kes_hucre = lambda i: ahmet.locator("tr").nth(i).locator("td.kes")
    assert kes_hucre(0).inner_text() == "Kesinleşti", kes_hucre(0).inner_text()
    assert kes_hucre(1).inner_text() == "Kesinleşme Bekliyor", kes_hucre(1).inner_text()
    # Kesinleşme taşıyanlar önce (2026/7574: banka yazıları ödeme emrini gizliyordu); kesinleşmesi olmayanlar sonda
    kesler = [kes_hucre(i).inner_text() for i in range(5)]
    ilk_bos = kesler.index("—")
    assert all(k != "—" for k in kesler[:ilk_bos]) and all(k == "—" for k in kesler[ilk_bos:]), kesler
    assert kes_hucre(4).locator("span").get_attribute("title") == "Bu tebligat türünde kesinleşme yok"
    assert s.locator("#cu-icra tbody.tik").nth(4).locator("td.kes").first.inner_text() == "İtiraz", "itirazda ödeme emri satırı İtiraz"
    s.screenshot(path=os.path.join(CIKTI, "icra1_liste.png"))

    # 2) Kutuya tıklayınca adetli seçenekler
    kutu("tur").click(); bekle(s, 150)
    sec = s.locator("#cu-icra .i-acilir .sec").all_inner_texts()
    assert sec[0].strip() == "Tümünü seç" and sec[1].split("\n") == ["Ödeme Emri", "4"], sec
    assert len(sec) == 5, sec
    s.screenshot(path=os.path.join(CIKTI, "icra2_acilir.png"))

    # 3) Yazarak süzme (içinde geçen, Türkçe-duyarsız) — odak ve yazı korunur
    kutu("tur").type("odeme", delay=40); bekle(s, 700)
    assert istekler[-1] == {"tur": "odeme"}, istekler[-1]
    assert say() == 4, say()
    assert kutu("tur").input_value() == "odeme"
    assert s.evaluate("document.activeElement === document.querySelector('#cu-icra .i-kutu-s[data-k=tur] input')"), "odak kaybolmamalı"
    assert s.locator("#cu-icra .i-acilir .sec").count() == 2, s.locator("#cu-icra .i-acilir .sec").all_inner_texts()  # Hepsi + Ödeme Emri
    assert "geçen" in s.locator("#cu-icra .i-secili").inner_text()

    # 4) Listeden seçme: tam eşleşme; uymayan tebligatlar soluk, uyanlar önde
    s.locator("#cu-icra .i-acilir .sec", has_text="Ödeme Emri").dispatch_event("mousedown"); bekle(s)
    assert istekler[-1] == {"tur": "=Ödeme Emri"}, istekler[-1]
    assert s.locator("#cu-icra .i-acilir").count() == 1, "çoklu seçim: liste açık kalır"
    assert s.locator("#cu-icra .i-acilir .sec.on").all_inner_texts()[0].split("\n")[:2] == ["✓", "Ödeme Emri"]
    kutu("tur").press("Escape"); bekle(s, 200)
    assert kutu("tur").input_value() == "Ödeme Emri"
    assert s.locator("#cu-icra .i-acilir").count() == 0
    ahmet = s.locator("#cu-icra tbody.tik").first
    ilk = ahmet.locator("tr").first.locator("td.tb0")
    assert "Ödeme Emri" in ilk.inner_text() and "sol" not in (ilk.get_attribute("class") or "")
    assert ahmet.locator("td.tb.sol").count() > 0, "uymayan tebligatlar soluk olmalı"

    # 5) İkinci süzgeç: seçenekler diğerine göre daralır; aynı tebligatta birlikte
    kutu("teblig").click(); bekle(s, 150)
    sec = [x.split("\n")[0] for x in s.locator("#cu-icra .i-acilir .sec").all_inner_texts()]
    assert "Henüz sorgulanmadı" not in sec and "İADE" in sec, sec     # Ödeme Emri'lerde sorgulanmamış yok
    s.locator("#cu-icra .i-acilir .sec", has_text="İADE").dispatch_event("mousedown"); bekle(s)
    assert istekler[-1] == {"tur": "=Ödeme Emri", "teblig": "=İADE"}, istekler[-1]
    assert say() == 2, say()                                           # Ahmet (5. tebligat) + Mehmet
    assert s.locator("#cu-icra .i-secili .et").count() == 2
    s.screenshot(path=os.path.join(CIKTI, "icra3_iki_suzgec.png"))

    # 6) Mazbata (kodlu): yazınca yalnız liste daralır, Enter ilk seçeneği seçer
    kutu("mazbata").click(); bekle(s, 150)
    kutu("mazbata").type("dön", delay=40); bekle(s, 500)
    assert "mazbata" not in istekler[-1], "kodlu kutuda yazı sunucuya gitmez"
    kutu("mazbata").press("ArrowDown"); kutu("mazbata").press("Enter"); bekle(s)
    assert istekler[-1].get("mazbata") in ("var", "yok"), istekler[-1]
    secilen = istekler[-1]["mazbata"]
    kutu("mazbata").press("Escape"); bekle(s, 200)
    assert kutu("mazbata").input_value() == ("Döndü" if secilen == "var" else "Dönmedi")

    # 7) Etiketten kaldırma ve "Hepsini kaldır"
    s.locator("#cu-icra .i-secili .et", has_text="Mazbata").click(); bekle(s)
    assert "mazbata" not in istekler[-1]
    s.locator("#cu-icra .i-secili .i-dahasi").click(); bekle(s)
    assert istekler[-1] == {}, istekler[-1]
    assert say() == 5 and s.locator("#cu-icra .i-secili").count() == 0

    # 8) Kesinleşme + aşama şeridi birlikte; ✕ ile kutudan kaldırma; boş sonuçta başlıklar yerinde
    kutu("kesinlesme").click(); bekle(s, 150)
    s.locator("#cu-icra .i-acilir .sec", has_text="Henüz hesaplanmadı").dispatch_event("mousedown"); bekle(s)
    assert istekler[-1] == {"kesinlesme": "-"} and say() == 2
    s.locator("#cu-icra .i-asama", has_text="Kesinleşti").click(); bekle(s)
    assert say() == 0 and s.locator("#cu-icra th.suz .i-kutu-s").count() == 8, "boş sonuçta da başlık kutuları durmalı"
    assert "uyan dosya yok" in s.locator("#cu-icra table.liste").inner_text()
    s.locator('#cu-icra .i-kutu-s[data-k="kesinlesme"] .sil').dispatch_event("mousedown"); bekle(s)
    assert istekler[-1] == {"asama": "kesinlesti"} and say() == 1

    # 9) Escape kutuyu kapatır, satıra tıklama dosya panelini açmaya çalışır (uç boş döner → hata vermez)
    kutu("tur").click(); bekle(s, 150)
    kutu("tur").press("Escape"); bekle(s, 150)
    assert s.locator("#cu-icra .i-acilir").count() == 0

    s.locator("#cu-icra .i-secili .et", has_text="Aşama").click(); bekle(s)
    assert istekler[-1] == {} and say() == 5

    # 10) Künye süzgeçleri: varsayılan açık dosyalar; daire yazarak; alacaklı listeden; yıl ve tür (üst kutular)
    assert son_sorgu.get("durum") == "acik", son_sorgu
    assert s.locator("#cu-icra .i-suzgecler .i-kutu-s").count() == 2, "geniş ekranda üstte yıl + tür"
    kutu("birim").click(); bekle(s, 150)
    sec = [x.split("\n") for x in s.locator("#cu-icra .i-acilir .sec").all_inner_texts()]
    assert sec == [["Tümünü seç"], ["İzmir 5. İcra Dairesi", "3"], ["Ankara 3. İcra Dairesi", "2"]], sec
    kutu("birim").type("ankara", delay=30); bekle(s, 700)
    assert istekler[-1] == {"birim": "ankara"} and say() == 2, (istekler[-1], say())
    kutu("birim").press("Escape"); bekle(s, 150)
    kutu("alacakli").click(); bekle(s, 150)
    s.locator("#cu-icra .i-acilir .sec", has_text="Y FİNANS").dispatch_event("mousedown"); bekle(s)
    assert istekler[-1] == {"birim": "ankara", "alacakli": "=Y FİNANS A.Ş."} and say() == 2, istekler[-1]
    kutu("dosya_tur").click(); bekle(s, 150)
    assert [x.split("\n")[0] for x in s.locator("#cu-icra .i-acilir .sec").all_inner_texts()] == ["MTS"]
    s.locator("#cu-icra .i-acilir .sec", has_text="MTS").dispatch_event("mousedown"); bekle(s)
    assert istekler[-1]["dosya_tur"] == "MTS" and kutu("dosya_tur").get_attribute("placeholder") == "MTS"
    # Kapalı dosyalar: borçlusuz dosya tek satır
    s.select_option("#cu-icra select[aria-label='Dosya durumu']", "kapali"); bekle(s)
    assert son_sorgu.get("durum") == "kapali" and say() == 1, (son_sorgu, say())
    satir = s.locator("#cu-icra tbody.tik").first.inner_text()
    assert "Borçlu kaydı yok" in satir and "henüz taranmadı" in satir and "açılış 15.03.2025" in satir, satir
    s.screenshot(path=os.path.join(CIKTI, "icra5_kunye.png"))
    # Açılış tarihi aralığı + etiket
    s.fill("#cu-icra input[aria-label='Açılış tarihi: başlangıç']", "2025-03-16"); bekle(s)
    assert istekler[-1]["acilis_bas"] == "2025-03-16" and say() == 0
    assert "16.03.2025" in s.locator("#cu-icra .i-secili").inner_text()
    s.locator("#cu-icra .i-secili .i-dahasi").click(); bekle(s)
    assert istekler[-1] == {} and son_sorgu.get("durum") == "kapali", "Hepsini kaldır durum seçimine dokunmaz"
    s.select_option("#cu-icra select[aria-label='Dosya durumu']", "acik"); bekle(s)
    assert say() == 5
    # Birden çok seçim: iki daire birlikte; etiket ikisini de yazar; biri bırakılınca kalan
    kutu("birim").click(); bekle(s, 150)
    s.locator("#cu-icra .i-acilir .sec", has_text="İzmir 5").dispatch_event("mousedown"); bekle(s)
    assert istekler[-1] == {"birim": "=İzmir 5. İcra Dairesi"} and say() == 3, (istekler[-1], say())
    s.locator("#cu-icra .i-acilir .sec", has_text="Ankara 3").dispatch_event("mousedown"); bekle(s)
    assert istekler[-1] == {"birim": "=İzmir 5. İcra Dairesi\nAnkara 3. İcra Dairesi"} and say() == 5, istekler[-1]
    assert s.locator("#cu-icra .i-acilir .sec.on").count() == 2
    assert kutu("birim").input_value() == "" and kutu("birim").get_attribute("placeholder") == "İzmir 5. İcra Dairesi +1"
    et = s.locator("#cu-icra .i-secili").inner_text()
    assert "İzmir 5. İcra Dairesi, Ankara 3. İcra Dairesi" in et, et
    s.screenshot(path=os.path.join(CIKTI, "icra6_coklu.png"))
    kutu("birim").type("izm", delay=30); bekle(s, 600)
    assert istekler[-1]["birim"].count("\n") == 1, "seçim varken yazı yalnız listeyi daraltır"
    assert [x.split("\n")[1] for x in s.locator("#cu-icra .i-acilir .sec.on").all_inner_texts()] == ["İzmir 5. İcra Dairesi"]
    s.locator("#cu-icra .i-acilir .sec.on", has_text="İzmir 5").dispatch_event("mousedown"); bekle(s)
    assert istekler[-1] == {"birim": "=Ankara 3. İcra Dairesi"} and say() == 2, istekler[-1]
    kutu("birim").press("Escape"); bekle(s, 200)
    kutu("yil").click(); bekle(s, 150)
    s.locator("#cu-icra .i-acilir .sec", has_text="2026").dispatch_event("mousedown"); bekle(s)
    assert istekler[-1] == {"birim": "=Ankara 3. İcra Dairesi", "yil": "2026"}, istekler[-1]
    kutu("yil").press("Escape"); bekle(s, 200)
    s.locator("#cu-icra .i-secili .i-dahasi").click(); bekle(s)
    assert istekler[-1] == {} and say() == 5

    # 10b) Daire ve dosya no ayrı sütunlar; "Tümünü seç" sonra bir daireyi kaldır (süzgeç "hariç" olur)
    basliklar = [x.split("\n")[0] for x in s.locator("#cu-icra table.liste thead th").all_inner_texts()]
    assert basliklar == ["İcra dairesi", "Dosya no", "Alacaklı", "Borçlu", "Kesinleşme", "Takip aşaması", "Tebligat türü", "Tebliğ durumu", "Mazbata"], basliklar
    ilk_satir = s.locator("#cu-icra tbody.tik").first.locator("tr").first.locator("td")
    assert ilk_satir.nth(0).inner_text().strip() == "İzmir 5. İcra Dairesi" and ilk_satir.nth(1).inner_text().startswith("2026/101"), ilk_satir.nth(1).inner_text()
    kutu("birim").click(); bekle(s, 150)
    s.locator("#cu-icra .i-acilir .sec", has_text="Tümünü seç").dispatch_event("mousedown"); bekle(s)
    assert istekler[-1] == {"birim": "!"} and say() == 5, istekler[-1]
    assert s.locator("#cu-icra .i-acilir .sec.on").count() == 2, "tümü işaretli"
    assert s.locator("#cu-icra .i-acilir .sec", has_text="Tümünü seç").count() == 0, "hepsi seçiliyken Tümünü seç gizlenir"
    s.locator("#cu-icra .i-acilir .sec", has_text="Ankara 3").dispatch_event("mousedown"); bekle(s)
    assert istekler[-1] == {"birim": "!Ankara 3. İcra Dairesi"} and say() == 3, (istekler[-1], say())
    assert [x.split("\n")[1] for x in s.locator("#cu-icra .i-acilir .sec.on").all_inner_texts()] == ["İzmir 5. İcra Dairesi"]
    assert s.locator("#cu-icra .i-acilir .sec:not(.on)", has_text="Ankara 3").count() == 1, "kaldırılan daire listede kalır, geri eklenebilir"
    kutu("birim").press("Escape"); bekle(s, 200)
    assert kutu("birim").input_value() == "Tümü (1 hariç)", kutu("birim").input_value()
    assert "tümü, hariç: Ankara 3. İcra Dairesi" in s.locator("#cu-icra .i-secili").inner_text()
    s.locator("#cu-icra .i-secili .et").click(); bekle(s)
    assert istekler[-1] == {} and say() == 5
    # Kodlu kutuda da: dosya türü tümü, biri hariç
    kutu("dosya_tur").click(); bekle(s, 150)
    s.locator("#cu-icra .i-acilir .sec", has_text="Tümünü seç").dispatch_event("mousedown"); bekle(s)
    s.locator("#cu-icra .i-acilir .sec", has_text="MTS").dispatch_event("mousedown"); bekle(s)
    assert istekler[-1] == {"dosya_tur": "!MTS"} and say() == 3, (istekler[-1], say())
    kutu("dosya_tur").press("Escape"); bekle(s, 200)
    s.locator("#cu-icra .i-secili .et").click(); bekle(s)
    assert istekler[-1] == {} and say() == 5
    # Dosya no süzgeci: yazınca içinde geçen
    kutu("dosya_no").click(); bekle(s, 150)
    kutu("dosya_no").type("2026/103", delay=30); bekle(s, 700)
    assert istekler[-1] == {"dosya_no": "2026/103"} and say() == 1, (istekler[-1], say())
    kutu("dosya_no").press("Escape"); bekle(s, 150)
    s.locator("#cu-icra .i-secili .i-dahasi").click() if s.locator("#cu-icra .i-secili .i-dahasi").count() else s.locator("#cu-icra .i-secili .et").click()
    bekle(s)
    assert istekler[-1] == {} and say() == 5

    # 11) UYAP'tan güncelleme: panel, başlatma, ilerleme, doğrulama, bitiş raporu, durdurma
    gtd = s.locator("#cu-icra .i-gt-dugme")
    assert gtd.inner_text() == "Toplu tarama" and s.locator("#cu-icra .i-gt").count() == 0
    gtd.click(); bekle(s, 300)
    panel = s.locator("#cu-icra .i-gt")
    assert panel.count() == 1
    secenek = panel.locator("select option").all_inner_texts()
    assert secenek == ["Bütün açık dosyalar", "Ankara 3. İcra Dairesi (2 açık)", "İzmir 5. İcra Dairesi (3 açık)"], secenek
    GT["sira"] = [gt_tarama(asama="taraf", i=3, toplam=10, kalan_sn=125, alinan=6515, yeni_dosya=10, mesaj="Taraflar alınıyor: X 2026/1 (3/10)")]
    liste_once = len(istekler)
    panel.locator("button", has_text="Dosya listesini güncelle").click(); bekle(s, 2600)
    assert GT["postlar"][-1] == ("", {"mod": "liste"}), GT["postlar"]
    panel = s.locator("#cu-icra .i-gt")
    metin = panel.inner_text()
    assert "Dosya listesi güncelleniyor" in metin and "Taraflar ve borçlu kimlikleri alınıyor" in metin and "3 / 10" in metin and "≈ 2 dk" in metin and "6515 dosya alındı" in metin, metin
    assert s.locator("#cu-icra .i-gt-dugme").inner_text() == "Taranıyor · 30%"
    assert len(istekler) > liste_once, "tarama sürerken yazılan dosyalar listede görünsün diye liste yenilenir"
    assert panel.locator("button", has_text="Kapat").count() == 0, "sürerken panel kapatılamaz"
    s.screenshot(path=os.path.join(CIKTI, "icra7_guncelleme.png"))
    # Doğrulama istenir: kutu çizilir, betik yüklenemezse bilgi; "Tekrar dene" jetonsuz gönderir
    GT["sira"] = [gt_tarama(durum="captcha", asama="taraf", i=4, toplam=10, captcha={"n": 1, "site_key": "x", "mesaj": "UYAP doğrulama istiyor."})]
    bekle(s, 2600)
    panel = s.locator("#cu-icra .i-gt")
    assert "UYAP doğrulama istiyor." in panel.inner_text() and panel.get_attribute("data-captcha") == "1", panel.inner_text()
    GT["sira"] = [gt_tarama(durum="captcha", asama="taraf", i=4, toplam=10, captcha={"n": 1, "site_key": "x", "mesaj": "UYAP doğrulama istiyor."})]
    bekle(s, 2600)
    assert s.locator("#cu-icra .i-gt").get_attribute("data-captcha") == "1", "doğrulama açıkken panel yeniden çizilmez"
    GT["sira"] = [gt_tarama(asama="kapanan", i=1, toplam=2)]
    s.locator("#cu-icra .i-gt button", has_text="Tekrar dene").click(); bekle(s, 600)
    assert GT["postlar"][-1] == ("/t1/captcha", {"token": None}), GT["postlar"][-1]
    # Bitiş: rapor gösterilir, liste yenilenir
    GT["sira"] = [gt_tarama(durum="bitti", asama="bitti", mesaj="Tamamlandı: 6515 açık dosya güncellendi (10 yeni, 3 kapandı).",
                            rapor=["✔ İcra (Bütün açık dosyalar): UYAP 6515 dosya bildirdi, 6515 dosyanın tamamı alındı.", "Durum hesabı: 12 borçlunun durumu değişti, 2 otomatik iş açıldı."],
                            uyari_sayisi=1, uyarilar=["✔ İzmir 5. İcra Dairesi 2026/9 kapalı olarak güncellendi (Kapalı)."])]
    bekle(s, 2600)
    panel = s.locator("#cu-icra .i-gt")
    metin = panel.inner_text()
    assert "Son güncelleme: dosya listesi tamamlandı" in metin and "6515 dosyanın tamamı alındı" in metin and "1 uyarı / not" in metin, metin
    assert s.locator("#cu-icra .i-gt-dugme").inner_text() == "Toplu tarama"
    assert len(istekler) > liste_once, "bitince liste yenilenir"
    s.screenshot(path=os.path.join(CIKTI, "icra8_guncelleme_bitti.png"))
    # Tek daire + tarih aralığı ile taraf bilgileri, sonra durdur
    panel.locator("select").select_option("2002")
    panel.locator("input[aria-label='Açılış tarihi: başlangıç']").fill("2026-11-01")
    panel.locator("input[aria-label='Açılış tarihi: bitiş']").fill("2026-11-30")
    GT["sira"] = [gt_tarama(mod="taraf", asama="liste", kapsam="Ankara 3. İcra Dairesi", mesaj="2 dosya alındı")]
    panel.locator("button", has_text="Taraf bilgilerini güncelle").click(); bekle(s, 600)
    assert GT["postlar"][-1] == ("", {"mod": "taraf", "birim_id": "2002", "birim_adi": "Ankara 3. İcra Dairesi",
                                       "acilis_bas": "2026-11-01", "acilis_son": "2026-11-30"}), GT["postlar"][-1]
    assert "Taraf bilgileri güncelleniyor" in s.locator("#cu-icra .i-gt").inner_text()
    s.locator("#cu-icra .i-gt button", has_text="Durdur").click(); bekle(s, 600)
    assert GT["postlar"][-1][0] == "/t1/iptal"
    metin = s.locator("#cu-icra .i-gt").inner_text()
    assert "Son güncelleme: taraf bilgileri durduruldu" in metin, metin
    # Kapsam seçimi yeniden çizimde korunur; "Bu ay" bu ayın ilk gününü yazar
    assert s.locator("#cu-icra .i-gt select").input_value() == "2002"
    assert s.locator("#cu-icra .i-gt input[aria-label='Açılış tarihi: bitiş']").input_value() == "2026-11-30"
    s.locator("#cu-icra .i-gt button", has_text="Bu ay").click()
    assert s.locator("#cu-icra .i-gt input[aria-label='Açılış tarihi: başlangıç']").input_value().endswith("-01")
    s.screenshot(path=os.path.join(CIKTI, "icra9_guncelleme_form.png"))
    # Tebligat bilgileri, PTT kapalı: gövdede ptt=false; sayaçlar görünür; durdurulunca başlık doğru
    bas = s.locator("#cu-icra .i-gt input[aria-label='Açılış tarihi: başlangıç']").input_value()
    son = s.locator("#cu-icra .i-gt input[aria-label='Açılış tarihi: bitiş']").input_value()
    kutu = s.locator("#cu-icra .i-gt input[aria-label='PTT durumunu da sorgula']")
    assert kutu.is_checked(), "PTT sorgusu varsayılan açık"
    kutu.uncheck()
    GT["sira"] = [gt_tarama(mod="tebligat", asama="tebligat", kapsam="Ankara 3. İcra Dairesi", i=1, toplam=4, alinan=4,
                            pdf_indirilen=3, teb_yazilan=3, teb_yeni=2, teb_degisen=1, mesaj="Tebligatlar: Ankara 3. İcra Dairesi 2026/5 (1/4)")]
    s.locator("#cu-icra .i-gt button", has_text="Tebligat bilgilerini güncelle").click(); bekle(s, 600)
    assert GT["postlar"][-1] == ("", {"mod": "tebligat", "ptt": False, "birim_id": "2002", "birim_adi": "Ankara 3. İcra Dairesi",
                                       "acilis_bas": bas, "acilis_son": son}), GT["postlar"][-1]
    bekle(s, 2400)
    metin = s.locator("#cu-icra .i-gt").inner_text()
    assert "Tebligat bilgileri güncelleniyor" in metin and "3 tebligat belgesi" in metin and "3 tebligat yazıldı (2 yeni, 1 durumu değişti)" in metin, metin
    s.screenshot(path=os.path.join(CIKTI, "icra10_tebligat.png"))
    s.locator("#cu-icra .i-gt button", has_text="Durdur").click(); bekle(s, 600)
    assert "Son güncelleme: tebligat bilgileri durduruldu" in s.locator("#cu-icra .i-gt").inner_text()
    assert not s.locator("#cu-icra .i-gt input[aria-label='PTT durumunu da sorgula']").is_checked(), "seçim yeniden çizimde korunur"
    # Takip türü bilgisi (eksikler): kapsamla gider; sayaç görünür
    GT["sira"] = [gt_tarama(mod="takip", asama="takip", kapsam="Ankara 3. İcra Dairesi", i=2, toplam=8, alinan=8, takip_okunan=2, takip_okunamayan=1)]
    s.locator("#cu-icra .i-gt button", has_text="Takip türü bilgisi").click(); bekle(s, 600)
    assert GT["postlar"][-1][1]["mod"] == "takip" and GT["postlar"][-1][1]["birim_id"] == "2002", GT["postlar"][-1]
    bekle(s, 2400)
    metin = s.locator("#cu-icra .i-gt").inner_text()
    assert "Takip türü bilgisi güncelleniyor" in metin and "Takip türü bilgileri alınıyor" in metin and "2 dosyanın takip türü okundu (1 alınamadı)" in metin, metin
    s.locator("#cu-icra .i-gt button", has_text="Durdur").click(); bekle(s, 600)
    # Yalnız PTT: UYAP'a gitmez, gövdede ptt alanı yok
    GT["sira"] = [gt_tarama(mod="ptt", asama="ptt", i=2, toplam=5, ptt_sorulan=2, teb_yazilan=2, teb_degisen=1)]
    s.locator("#cu-icra .i-gt button", has_text="Yalnız PTT").click(); bekle(s, 600)
    assert GT["postlar"][-1][1]["mod"] == "ptt" and "ptt" not in GT["postlar"][-1][1], GT["postlar"][-1]
    bekle(s, 2400)
    metin = s.locator("#cu-icra .i-gt").inner_text()
    assert "PTT durumları güncelleniyor" in metin and "2 PTT sorgusu" in metin and "dosya alındı" not in metin, metin
    s.locator("#cu-icra .i-gt button", has_text="Durdur").click(); bekle(s, 600)
    s.locator("#cu-icra .i-gt button", has_text="Kapat").click(); bekle(s, 200)
    assert s.locator("#cu-icra .i-gt").count() == 0

    # 11b) Dosya paneli: tek dosyayı UYAP'tan güncelle (künye + taraf + bütün tebligatlar), sonra seçilen tebligatı sorgula
    s.locator("#cu-icra tbody.tik").first.locator("td").first.click(); bekle(s, 600)
    panel = s.locator("#cu-icra .i-panel")
    assert panel.count() == 1 and "2026/101" in panel.inner_text()
    DI["sira"] = [gt_tarama(id="d1", mod="dosya", asama="tebligat", dosya_kesinlesme=True, mesaj="İzmir 5. İcra Dairesi 2026/101: tebligatlar alınıyor…"),
                  gt_tarama(id="d1", mod="dosya", durum="bitti", asama="bitti", dosya_kesinlesme=True,
                            mesaj="İzmir 5. İcra Dairesi 2026/101 güncellendi: 2 tebligat (0 yeni, 1 durumu değişti) — 1 borçlu kesinleştirilebilir.",
                            rapor=["✔ Künye güncellendi: İzmir 5. İcra Dairesi 2026/101 (Açık)", "✔ Taraflar güncellendi.",
                                   "✔ Kesinleşme UYAP'tan okundu: 0 borçlu UYAP'ta kesinleşmiş.",
                                   "1 borçlu kesinleştirilebilir: talep göndermek için onayınız bekleniyor."],
                            talep_adaylari=[{"bag": 11, "borclu": "AHMET YILMAZ", "grup": "kesinlestirilebilir", "grup_etiket": "Kesinleştirilebilir"}])]
    panel.locator("button", has_text="Veritabanını güncelle").click(); bekle(s, 300)
    assert DI["postlar"][-1] == {"dosya": 1, "taraf": True, "tebligat": True, "kesinlesme": True}, DI["postlar"][-1]
    assert "Veritabanı güncelleniyor" in s.locator("#cu-icra .i-dosya-is").inner_text()
    assert s.locator("#cu-icra .i-dosya-guncelle").is_disabled(), "iş sürerken düğme kapalı"
    bekle(s, 3600)
    metin = s.locator("#cu-icra .i-dosya-is").inner_text()
    assert "güncellendi: 2 tebligat" in metin and "Künye güncellendi" in metin and "Kesinleşme UYAP'tan okundu" in metin, metin
    assert not s.locator("#cu-icra .i-dosya-guncelle").is_disabled()
    # Talep onay kutusu: onaysız hiçbir şey gönderilmez; kutu işaretleri seçimi değiştirir
    talep = s.locator("#cu-icra .i-panel-talep")
    assert talep.count() == 1 and "AHMET YILMAZ" in talep.inner_text() and "geri alınamaz" in talep.inner_text(), talep.inner_text()
    onceki_gt = len(GT["postlar"])
    gonder = talep.locator("button", has_text="E-imzala ve gönder")
    assert "(1 borçlu)" in gonder.inner_text()
    talep.locator("input[type=checkbox]").uncheck(); bekle(s, 100)
    assert s.locator("#cu-icra .i-panel-talep button", has_text="E-imzala ve gönder").is_disabled(), "seçim yoksa gönderilmez"
    s.locator("#cu-icra .i-panel-talep input[type=checkbox]").check(); bekle(s, 100)
    assert len(GT["postlar"]) == onceki_gt, "onaysız gönderilmez"
    s.screenshot(path=os.path.join(CIKTI, "icra11_panel_talep.png"))
    s.locator("#cu-icra .i-panel-talep button", has_text="E-imzala ve gönder").click(); bekle(s, 500)
    assert GT["postlar"][-1] == ("", {"mod": "kesinlestirme_talebi", "dosyalar": [1], "borclular": [11]}), GT["postlar"][-1]
    assert "Talep işi başladı" in s.locator("#cu-icra .i-panel-talep").inner_text()
    GT["son"] = gt_tarama(mod="kesinlestirme_talebi", durum="bitti", asama="bitti", mesaj="Tamamlandı: 1 kesinleştirme talebi gönderildi.")
    bekle(s, 2600)
    gt_panel = s.locator("#cu-icra .i-gt:not(.i-dosya-is)")
    assert "1 kesinleştirme talebi gönderildi" in gt_panel.inner_text(), gt_panel.inner_text()
    gt_panel.locator("button", has_text="Kapat").click(); bekle(s, 200)
    assert s.locator("#cu-icra .i-gt:not(.i-dosya-is)").count() == 0
    s.locator("#cu-icra .i-panel .i-sekmeler button", has_text="Tebligatlar").click(); bekle(s, 200)
    secili = s.locator("#cu-icra .i-panel button", has_text="Seçilenleri sorgula")
    assert secili.is_disabled() and "(0)" in secili.inner_text()
    s.locator("#cu-icra .i-panel input[type=checkbox]").first.check()
    assert "(1)" in secili.inner_text() and not secili.is_disabled()
    DI["sira"] = [gt_tarama(id="d1", mod="dosya", asama="tebligat", mesaj="tebligat")]
    secili.click(); bekle(s, 300)
    assert DI["postlar"][-1] == {"dosya": 1, "taraf": False, "tebligat": True, "evraklar": [5]}, DI["postlar"][-1]
    s.screenshot(path=os.path.join(CIKTI, "icra11_dosya_guncelle.png"))
    s.locator("#cu-icra .i-dosya-is button", has_text="Durdur").click(); bekle(s, 600)
    s.evaluate("() => { window.__uyapSagMenuAc = function(x, y, g) { window.__menu = g; window.__menuMetin = g.map(function(gr) { return gr.map(function(m) { return [m[0], !!m[1]]; }); }); }; }")
    s.locator("#cu-icra .i-panel .i-sekmeler button", has_text="Tebligatlar").click(); bekle(s, 200)
    assert s.locator("#cu-icra .i-panel .bolum", has_text="Mazbata evrakları (1)").count() == 1
    s.locator("#cu-icra .i-panel .i-kart", has_text="Ödeme Emri").first.click(button="right"); bekle(s, 100)
    menu = s.evaluate("window.__menuMetin")
    assert menu[0][0] == ["Mazbata bağla · 4007663500001", False] and menu[0][1] == ["Kapalı Tebliğ Mazbatası · 20/10/2026", True], menu
    assert len(menu) == 1, "elle bağ yokken 'kaldır' yok"
    # Bağlanınca panel yeniden okunur (sunucu artık bağı döndürür)
    DOSYA["mazbatalar"][0]["elle_tebligat"] = 5
    DOSYA["tebligatlar"][0].update(mazbata_var=True, mazbata="Elle bağlandı: Kapalı Tebliğ Mazbatası (evrak 9, 20.10.2026)", mazbata_elle=True)
    s.evaluate("window.__menu[0][1][1]()"); bekle(s, 600)
    assert MZ_POST[-1] == {"dosya": 1, "tebligat": 5, "mazbata": 9}, MZ_POST
    assert "Elle bağlandı: Kapalı Tebliğ Mazbatası" in s.locator("#cu-icra .i-panel .i-kart", has_text="Ödeme Emri").first.inner_text()
    # Mazbata kartından: tebligat listesi; elle bağlıysa "Elle bağı kaldır"
    mkart = s.locator("#cu-icra .i-panel .i-mazbata")
    assert "Ödeme Emri · 4007663500001" in mkart.inner_text(), mkart.inner_text()
    mkart.click(button="right"); bekle(s, 100)
    menu = s.evaluate("window.__menuMetin")
    assert [m[0] for m in menu[0][1:]] == ["103 Davetiyesi · 4007663500002 · BORÇLU AHMET YILMAZ · 03/10/2026"] and menu[1] == [["Elle bağı kaldır", True]], menu
    s.evaluate("window.__menu[1][0][1]()"); bekle(s, 400)
    assert MZ_POST[-1] == {"dosya": 1, "tebligat": 5, "mazbata": None}, MZ_POST
    s.screenshot(path=os.path.join(CIKTI, "icra11_mazbata_bagi.png"))
    DOSYA["mazbatalar"][0]["elle_tebligat"] = None
    DOSYA["tebligatlar"][0].update(mazbata_var=False, mazbata="", mazbata_elle=False)
    # Borçlu kartı: Mernis bilgisi + sağ tık (Mernis sorgusu / ücretli yeniden tebliğ, onay kutusuyla)
    DOSYA["taraflar"] = [{"bag": 11, "rol": "borclu", "rol_etiket": "Borçlu", "ad": "AHMET YILMAZ", "tuzel": False, "kimlik": "11111111110",
                          "uyapta_gorulmedi": False, "vekiller": "", "asama": "teblig", "asama_metni": "Tebliğ", "kesinlesme_durumu": "",
                          "kesinlesme": "—", "tebligat": "İade (06.10.2026)", "teblig_kodu": "iade", "itiraz": False, "itiraz_tarihi": None,
                          "kesinlesme_elle_tarihi": None,
                          "mernis": {"kategori": "uygun", "etiket": "21/2 şerhli tebliğe uygun", "adres": "BOSTANLI MAH. 1720 SK. No:5 KARŞIYAKA/ İZMİR",
                                     "teblig_adresi": "Bostanlı Mah. 1720 Sk. No:5 Karşıyaka/ İzmir", "aciklama": "Ödeme emri Mernis adresine çıkmış, iade dönmüş",
                                     "zaman": "23.11.2026 19:05", "talep_turu": "T.K.21/2 Şerhli", "tutar": 265}}]
    s.locator("#cu-icra .i-panel .i-sekmeler button", has_text="Borçlular").click(); bekle(s, 150)
    s.locator("#cu-icra .i-panel button[aria-label='Paneli kapat']").click(); bekle(s, 200)
    s.locator("#cu-icra tbody.tik").first.locator("td").first.click(); bekle(s, 600)
    s.locator("#cu-icra .i-panel .i-sekmeler button", has_text="Borçlular").click(); bekle(s, 200)
    kart = s.locator("#cu-icra .i-panel .i-borclu")
    assert "21/2 şerhli tebliğe uygun" in kart.inner_text() and "Mernis adresi: BOSTANLI MAH." in kart.inner_text(), kart.inner_text()
    assert s.locator("#cu-icra .i-panel button", has_text="Tebligatları güncelle").count() == 1
    kart.click(button="right"); bekle(s, 100)
    menu = s.evaluate("window.__menuMetin")
    assert menu[1][0] == ["Mernis adresini yeniden sorgula (son: 23.11.2026 19:05)", True], menu
    assert menu[2][0] == ["Yeniden tebliğ talebi gönder — T.K.21/2 Şerhli (≈ 265 TL)…", True], menu
    onceki_gt = len(GT["postlar"])
    s.evaluate("window.__menu[2][0][1]()"); bekle(s, 300)
    onay = s.locator("#cu-icra .i-yt-onay")
    assert onay.count() == 1 and "ÜCRETLİDİR" in onay.inner_text() and "İlk tebligat adresi: Bostanlı" in onay.inner_text(), onay.inner_text()
    assert onay.locator("button", has_text="E-imzala ve gönder").is_disabled() and len(GT["postlar"]) == onceki_gt, "onaysız gönderilmez"
    s.screenshot(path=os.path.join(CIKTI, "icra11_yeniden_teblig_panel.png"))
    onay.locator("input[type=checkbox]").check(); bekle(s, 150)
    s.locator("#cu-icra .i-yt-onay button", has_text="E-imzala ve gönder").click(); bekle(s, 500)
    assert GT["postlar"][-1] == ("", {"mod": "yeniden_teblig", "dosyalar": [1], "borclular": [11]}), GT["postlar"][-1]
    assert s.locator("#cu-icra .i-yt-onay").count() == 0
    GT["son"] = gt_tarama(mod="yeniden_teblig", durum="bitti", asama="bitti", mesaj="Tamamlandı: 1 yeniden tebliğ talebi gönderildi (≈ 265 TL).")
    bekle(s, 2600)
    # Mernis (yeniden) sorgusu menüden doğrudan başlar (salt okuma)
    s.locator("#cu-icra .i-panel .i-borclu").click(button="right"); bekle(s, 100)
    s.evaluate("window.__menu[1][0][1]()"); bekle(s, 500)
    assert GT["postlar"][-1] == ("", {"mod": "mernis", "dosyalar": [1], "borclular": [11], "yeniden": True}), GT["postlar"][-1]
    GT["son"] = gt_tarama(mod="mernis", durum="bitti", asama="bitti", mesaj="Tamamlandı: 1 borçlunun Mernis adresi alındı.")
    bekle(s, 2600)
    gt_panel = s.locator("#cu-icra .i-gt:not(.i-dosya-is)")
    if gt_panel.count():
        gt_panel.locator("button", has_text="Kapat").click(); bekle(s, 200)
    DOSYA["taraflar"] = []
    # Dosya sekmesi: takip türü, yolu, şekli adlarıyla (kod gösterilmez)
    DOSYA["takip"] = {"okundu": True, "turu": "1", "yolu": "0", "sekli": "0", "turu_ad": "İlamsız Takip", "yolu_ad": "Genel Haciz Yoluyla Takip",
                      "sekli_ad": "ÖRNEK: 7 İlamsız Takiplerde Ödeme Emri - Eski No: 49", "ornek_no": "7", "okunma": "2026-11-19 14:05",
                      "kesinlesme_suresi_gun": 7, "kural": "Varsayılan", "alacak_toplam": "1500.00"}
    s.locator("#cu-icra .i-panel button[aria-label='Paneli kapat']").click(); bekle(s, 200)
    s.locator("#cu-icra tbody.tik").first.locator("td").nth(2).click(); bekle(s, 600)
    s.locator("#cu-icra .i-panel .i-sekmeler button", has_text="Dosya").click(); bekle(s, 400)
    metin = s.locator("#cu-icra .i-panel").inner_text()
    assert "İlamsız Takip · Örnek 7" in s.locator("#cu-icra .i-panel .i-rozet").inner_text(), "rozet kod değil ad gösterir"
    assert "İlamsız Takip" in metin and "Genel Haciz Yoluyla Takip" in metin and "ÖRNEK: 7 İlamsız Takiplerde Ödeme Emri" in metin, metin
    assert "19.11.2026 14:05" in metin and "Takip kodu" not in metin, metin
    s.screenshot(path=os.path.join(CIKTI, "icra13_dosya_takip.png"))
    s.locator("#cu-icra .i-panel button[aria-label='Paneli kapat']").click(); bekle(s, 200)

    # 11c) Dosya seçimi: satır kutusu paneli açmaz; "Tümünü seç" süzgeçteki bütün dosyalar; hesap ve toplu tarama seçime işler
    sayi = lambda: s.locator("#cu-icra .i-secim-sayi").inner_text()
    assert sayi() == "Seçili dosya yok"
    s.locator("#cu-icra input.i-dosya-sec").first.check(); bekle(s, 200)
    assert s.locator("#cu-icra .i-panel").count() == 0, "kutu dosya panelini açmamalı"
    assert sayi() == "1 dosya seçili" and s.locator("#cu-icra .i-hesapla").inner_text() == "Yeniden hesapla (1 seçili)"
    s.locator("#cu-icra .i-hesapla").click(); bekle(s, 600)
    assert HESAP[-1] == {"dosyalar": [1]}, HESAP[-1]
    s.locator("#cu-icra .i-secim button", has_text="Tümünü seç").click(); bekle(s, 600)
    assert sayi() == "5 dosya seçili", sayi()
    assert s.locator("#cu-icra input.i-dosya-sec:checked").count() == s.locator("#cu-icra input.i-dosya-sec").count()
    s.locator("#cu-icra .i-gt-dugme").click(); bekle(s, 400)
    gt = s.locator("#cu-icra .i-gt")
    assert "Listede seçili 5 dosya" in gt.inner_text() and gt.locator("select").is_disabled(), gt.inner_text()
    GT["sira"] = [gt_tarama(mod="liste", asama="arama", kapsam="Seçili 5 dosya")]
    gt.locator("button", has_text="Dosya listesini güncelle").click(); bekle(s, 600)
    govde = GT["postlar"][-1][1]
    assert govde["mod"] == "liste" and sorted(govde["dosyalar"]) == [1, 2, 3, 4, 5] and "birim_id" not in govde, govde
    s.screenshot(path=os.path.join(CIKTI, "icra12_secim.png"))
    s.locator("#cu-icra .i-gt button", has_text="Durdur").click(); bekle(s, 600)
    s.locator("#cu-icra .i-secim button", has_text="Tümünü bırak").click(); bekle(s, 200)
    assert sayi() == "Seçili dosya yok" and s.locator("#cu-icra input.i-dosya-sec:checked").count() == 0
    assert "Listede seçili" not in s.locator("#cu-icra .i-gt").inner_text(), "seçim yokken kapsam seçeneği gizlenir"
    s.locator("#cu-icra .i-gt button", has_text="Kapat").click(); bekle(s, 200)

    # 13) Kesinleşme süreleri: takip türü → yolu → şekli adlarla seçilir; ad önerilir; kural tablosu ve dağılım adlarla
    s.locator("#cu-icra .i-sekmeler button", has_text="Kesinleşme Süreleri").click(); bekle(s, 500)
    ana = s.locator("#cu-icra .i-ana")
    assert "Kod (tür" not in ana.inner_text() and "Tür kodu" not in ana.inner_html(), "kod kutusu kalmamalı"
    tur_s, yol_s, sek_s = (ana.locator("select[aria-label='Takip %s']" % x) for x in ("türü", "yolu", "şekli"))
    assert tur_s.locator("option").all_inner_texts() == ["Bütün takip türleri (varsayılan)", "İlamlı Takip", "İlamsız Takip · 12 dosya"], tur_s.locator("option").all_inner_texts()
    assert yol_s.is_disabled() and sek_s.is_disabled()
    tur_s.select_option("1"); bekle(s, 100)
    assert yol_s.locator("option").all_inner_texts() == ["Bütün yollar", "Genel Haciz Yoluyla Takip · 12 dosya", "Kambiyo Senetlerine Mahsus Haciz Yoluyla Takip"]
    yol_s.select_option("0"); bekle(s, 100)
    assert sek_s.locator("option").all_inner_texts()[1] == "ÖRNEK: 7 İlamsız Takiplerde Ödeme Emri · 12 dosya", sek_s.locator("option").all_inner_texts()
    sek_s.select_option("0"); bekle(s, 100)
    ad = ana.locator("input[aria-label='Kuralın adı']")
    assert ad.input_value() == "İlamsız Takip · Genel Haciz Yoluyla Takip · Örnek 7", ad.input_value()
    # Tür değişince alt kademeler sıfırlanır
    tur_s.select_option("0"); bekle(s, 100)
    assert yol_s.input_value() == "" and sek_s.is_disabled() and ad.input_value() == "İlamlı Takip"
    tur_s.select_option("1"); yol_s.select_option("0"); sek_s.select_option("1"); bekle(s, 100)
    ana.locator("input[aria-label='Kesinleşme süresi (gün)']").fill("10")
    ana.locator("input[aria-label='İtiraz süresi (gün)']").fill("5")
    ana.locator("label", has_text="İtiraz takibi durdurur").locator("input").uncheck()
    ana.locator("input[aria-label='Dayanak']").fill("İİK 168")
    ana.locator("button", has_text="Ekle").click(); bekle(s, 500)
    assert KURAL_POST[-1] == {"id": None, "takibin_turu": "1", "takibin_yolu": "0", "takibin_sekli": "1",
                              "ad": "İlamsız Takip · Genel Haciz Yoluyla Takip · Örnek 10", "gun": 10, "aktif": True,
                              "itiraz_gun": 5, "itiraz_durdurur": False, "dayanak": "İİK 168"}, KURAL_POST[-1]
    # Kural tablosu: hazır kural silinmez; değiştirilmişse "Varsayılana dön"; değişmemişse düğme yok; itiraz etkisi ve dosya sayısı
    satirlar = s.locator("#cu-icra .i-ana table").first.locator("tbody tr")
    s1, s2 = satirlar.nth(0).inner_text(), satirlar.nth(1).inner_text()
    assert "Değiştirildi" in s1 and "40" in s1 and "Durdurur" in s1, s1
    assert "Varsayılan" in s2 and "10 gün" in s2 and "5 gün" in s2 and "Durdurmaz" in s2 and "İİK 168–170" in s2, s2
    assert satirlar.locator("button", has_text="Sil").count() == 0, "hazır kural silinemez"
    assert satirlar.nth(1).locator("button", has_text="Varsayılana dön").count() == 0
    satirlar.nth(0).locator("button", has_text="Varsayılana dön").click(); bekle(s, 500)
    assert KURAL_POST[-1] == {"varsayilan": {"id": 1}}, KURAL_POST[-1]
    # Düzenle: yeni alanlar forma dolar
    satirlar.nth(1).locator("button", has_text="Düzenle").click(); bekle(s, 200)
    assert ana.locator("input[aria-label='Kesinleşme süresi (gün)']").input_value() == "10"
    assert ana.locator("input[aria-label='İtiraz süresi (gün)']").input_value() == "5"
    assert not ana.locator("label", has_text="İtiraz takibi durdurur").locator("input").is_checked()
    assert ana.locator("input[aria-label='Dayanak']").input_value() == "İİK 168–170"
    ana.locator("button", has_text="Vazgeç").click(); bekle(s, 200)
    metin = s.locator("#cu-icra .i-ana").inner_text()
    assert "Bütün takipler (varsayılan)" in metin and "İlamsız Takip › Genel Haciz Yoluyla Takip › ÖRNEK: 7 İlamsız Takiplerde Ödeme Emri" in metin, metin
    assert "40 (bunlara varsayılan süre uygulanır)" in metin and "UYAP listesi henüz alınmadı" in metin, metin
    # "Süre gir": dağılım satırı forma adlarıyla gelir
    s.locator("#cu-icra .i-ana button", has_text="Süre gir").click(); bekle(s, 200)
    assert s.locator("#cu-icra select[aria-label='Takip şekli']").input_value() == "0"
    assert s.locator("#cu-icra input[aria-label='Kuralın adı']").input_value() == "İlamsız Takip · Genel Haciz Yoluyla Takip · Örnek 7"
    s.screenshot(path=os.path.join(CIKTI, "icra14_kesinlesme_sureleri.png"))
    # UYAP'tan takip türü listesi: toplu tarama ucuna gider, bitince sayfa yenilenir
    GT["sira"] = [gt_tarama(mod="takip_listesi", durum="bitti", asama="bitti", mesaj="Tamamlandı: UYAP takip türü listesi alındı (2 tür, 3 yol, 3 şekil).")]
    once = len(KURAL_GET)
    s.locator("#cu-icra .i-ana button", has_text="UYAP'tan takip türü listesini al").click(); bekle(s, 2500)
    assert GT["postlar"][-1] == ("", {"mod": "takip_listesi"}), GT["postlar"][-1]
    assert len(KURAL_GET) > once, "bitince kurallar yeniden yüklenir"
    s.locator("#cu-icra .i-sekmeler button", has_text="Süreçler").click(); bekle(s, 500)

    # 14) Tebligatlar: tebligat başına satır; bağlanmamış banka zarfı muhatap adıyla; sıralama, süzgeç, seçim, Excel, tür eşleştir
    s.locator("#cu-icra .i-sekmeler button", has_text="Tebligatlar").click(); bekle(s, 600)
    tablo = s.locator("#cu-icra table.teb")
    basliklar = [x.split("\n")[0].strip() for x in tablo.locator("thead th").all_inner_texts()]
    assert basliklar[1:4] == ["İcra dairesi", "Dosya no", "Dosya türü"] and "Teslim şekli / İade nedeni" in basliklar and "Muhatap türü" in basliklar, basliklar
    assert tablo.locator("tbody.teb-dosya").count() == 2 and tablo.locator("tbody.teb-dosya tr").count() == 3, "dosya başına grup, tebligat başına satır"
    banka = tablo.locator("tbody.teb-dosya").first.locator("tr").nth(1)
    assert "ARACIKISIKURUM DENİZBANK A.Ş." in banka.inner_text() and "Üçüncü kişi" in banka.inner_text(), banka.inner_text()
    assert banka.locator("td.tekrar").count() >= 4, "aynı dosyanın ikinci satırında dosya bilgileri soluk"
    talimat = tablo.locator("tbody.teb-dosya").nth(1).locator("td").last
    assert talimat.inner_text() == "—" and talimat.locator("span").get_attribute("title") == "Talimat dosyasında kesinleşme yok"
    # Sıralama: başlığa tıkla → artan, yeniden → azalan
    tablo.locator("thead .i-sirala", has_text="Tebligat türü").click(); bekle(s)
    assert TEB_ISTEK[-1] == {"sirala": "tur", "yon": "artan"}, TEB_ISTEK[-1]
    s.locator("#cu-icra table.teb thead .i-sirala", has_text="Tebligat türü").click(); bekle(s)
    assert TEB_ISTEK[-1] == {"sirala": "tur", "yon": "azalan"} and "▼" in s.locator("#cu-icra table.teb thead .i-sirala", has_text="Tebligat türü").inner_text()
    # Muhatap türü (kodlu) süzgeci: listeden "Üçüncü kişi"
    skutu = lambda k: s.locator('#cu-icra .i-kutu-s[data-k="%s"] input' % k)
    skutu("muhatap").click(); bekle(s, 150)
    s.locator("#cu-icra .i-acilir .sec", has_text="Üçüncü kişi").dispatch_event("mousedown"); bekle(s)
    assert TEB_ISTEK[-1].get("muhatap") == "ucuncu" and s.locator("#cu-icra table.teb tbody.teb-dosya tr").count() == 1, TEB_ISTEK[-1]
    skutu("muhatap").press("Escape"); bekle(s, 150)
    assert "Muhatap türü:" in s.locator("#cu-icra .i-secili").inner_text()
    s.locator("#cu-icra .i-secili .et").click(); bekle(s)
    # Evrak tarihi aralığı
    s.fill("#cu-icra input[aria-label='Evrak tarihi: başlangıç']", "2026-10-02"); bekle(s)
    assert TEB_ISTEK[-1].get("evrak_bas") == "2026-10-02", TEB_ISTEK[-1]
    assert "Evrak tarihi" in s.locator("#cu-icra .i-secili").inner_text()
    s.locator("#cu-icra .i-secili .et").click(); bekle(s)
    # Seçim: satır kutusu dosyayı seçer (aynı dosyanın bütün satırları işaretli), Tümünü seç süzgeçteki dosyalar
    s.locator("#cu-icra table.teb input.i-dosya-sec").first.check(); bekle(s, 200)
    assert s.locator("#cu-icra table.teb input.i-dosya-sec:checked").count() == 2 and s.locator("#cu-icra .i-panel").count() == 0
    s.locator("#cu-icra .i-secim button", has_text="Tümünü seç").click(); bekle(s, 500)
    assert s.locator("#cu-icra .i-secim-sayi").inner_text() == "2 dosya seçili", s.locator("#cu-icra .i-secim-sayi").inner_text()
    s.locator("#cu-icra .i-secim button", has_text="Tümünü bırak").click(); bekle(s, 200)
    # Excel: süzgeç ve sıra gider, dosya bu cihaza iner
    with s.expect_download() as indir:
        s.locator("#cu-icra button", has_text="Excel'e aktar").click()
    assert indir.value.suggested_filename == "Tebligatlar_20261119_1600.xlsx", indir.value.suggested_filename
    assert EXCEL_ISTEK[-1] == {"durum": "acik", "sirala": "tur", "yon": "azalan"}, EXCEL_ISTEK[-1]
    # Satıra tıklama dosya panelini açar
    s.locator("#cu-icra table.teb tbody.teb-dosya").first.locator("td").nth(6).click(); bekle(s, 600)
    assert s.locator("#cu-icra .i-panel").count() == 1
    s.locator("#cu-icra .i-panel button[aria-label='Paneli kapat']").click(); bekle(s, 200)
    s.screenshot(path=os.path.join(CIKTI, "icra15_tebligatlar.png"))
    # Tür eşleştir: grupları işaretle, tür yaz, ata, kaydet
    s.locator("#cu-icra button", has_text="Tür eşleştir").click(); bekle(s, 500)
    pen = s.locator("#cu-icra .i-pencere")
    assert pen.locator("tbody tr").count() == 2 and "Tensip zaptı" in pen.inner_text() and "Mazbata" in pen.inner_text()
    assert pen.locator("tbody tr").first.locator("td").nth(1).get_attribute("title") == "Tensip zaptı (10)\nTENSİP ZAPTI (2)"
    pen.locator("tbody input[type=checkbox]").first.check()
    pen.locator("input[aria-label='Atanacak tür']").fill("Tensip Zaptı")
    pen.locator("button", has_text="Seçili gruplara ata").click(); bekle(s, 100)
    pen.locator("tbody input[type=checkbox]").nth(1).check()
    pen.locator("input[aria-label='Atanacak tür']").fill("Satış İlanı")
    pen.locator("button", has_text="Seçili gruplara ata").click(); bekle(s, 100)
    assert "2 gruba tür atandı" in pen.inner_text() and "Tensip Zaptı" in pen.locator("tbody tr").first.inner_text()
    s.screenshot(path=os.path.join(CIKTI, "icra16_tur_eslestir.png"))
    once = len(TEB_ISTEK)
    pen.locator("button", has_text="Kaydet ve uygula").click(); bekle(s, 600)
    assert TUR_POST[-1] == {"eslesmeler": [{"anahtar": "tensip zapti", "tur": "Tensip Zaptı"}, {"anahtar": "ilani satis", "tur": "Satış İlanı"}]}, TUR_POST[-1]
    assert s.locator("#cu-icra .i-pencere").count() == 0 and len(TEB_ISTEK) > once, "pencere kapanır, liste yenilenir"
    # 14b) Grup çubuğu (adetli, tıklayınca süzer, yeniden tıklayınca kalkar); tür altında zarf içeriği; 89 sırası sorulur
    gruplar = s.locator("#cu-icra .i-gruplar")
    assert gruplar.count() == 1, "grup çubuğu"
    cipler = [x.strip() for x in gruplar.locator(".i-grup").all_inner_texts()]
    assert cipler[:3] == ["Tümü 3", "Ödeme / İcra Emri 1", "Banka 1"] and "Kıymet Takdiri 1" in cipler, cipler
    gruplar.locator(".i-grup", has_text="Banka").click(); bekle(s, 400)
    assert TEB_ISTEK[-1].get("grup") == "Banka" and s.locator("#cu-icra table.teb tbody.teb-dosya tr").count() == 1, TEB_ISTEK[-1]
    assert "aktif" in s.locator("#cu-icra .i-gruplar .i-grup", has_text="Banka").get_attribute("class")
    # 89: belgede yazmıyor → "?" → seçenekler → 89/2 kaydedilir
    s.locator("#cu-icra table.teb tbody.teb-dosya button", has_text="?").click(); bekle(s, 150)
    secenek = [x.strip() for x in s.locator("#cu-icra table.teb .h89 button").all_inner_texts()]
    assert secenek == ["89/1", "89/2", "89/3"], secenek
    s.locator("#cu-icra table.teb .h89 button", has_text="89/2").click(); bekle(s, 400)
    assert HACIZ_POST[-1] == {"tebligat": 2, "sira": 2}, HACIZ_POST
    s.locator("#cu-icra .i-gruplar .i-grup", has_text="Banka").click(); bekle(s, 400)
    assert not TEB_ISTEK[-1].get("grup") and s.locator("#cu-icra table.teb tbody.teb-dosya tr").count() == 3, "aynı gruba yeniden tıklayınca süzgeç kalkar"
    odeme = s.locator("#cu-icra table.teb tbody.teb-dosya").first.locator("tr").first.inner_text()
    assert "Ödeme / İcra Emri" in odeme and "4-5 ÖRNEK İCRA EMRİ VE İLAM SURETİ" in odeme, odeme
    s.screenshot(path=os.path.join(CIKTI, "icra16_tebligat_gruplari.png"))
    s.locator("#cu-icra .i-sekmeler button", has_text="Süreçler").click(); bekle(s, 500)

    # 12) Dar ekran: kutular tablonun üstünde
    s.evaluate("window.__cuDar = function() { return true; }; document.documentElement.classList.add('cu-dar')")
    s.set_viewport_size({"width": 420, "height": 860})
    s.locator("#cu-icra .i-asama", has_text="Kesinleşti").click(); bekle(s)
    assert s.locator("#cu-icra .i-suzgecler .i-kutu-s").count() == 10 and s.locator("#cu-icra th.suz .i-kutu-s").count() == 0
    s.locator('#cu-icra .i-suzgecler .i-kutu-s[data-k="tur"] input').click(); bekle(s, 150)
    assert s.locator("#cu-icra .i-acilir .sec").count() >= 1
    kutu_r = s.locator("#cu-icra .i-acilir").bounding_box()
    assert kutu_r["x"] >= 0 and kutu_r["x"] + kutu_r["width"] <= 421, kutu_r
    s.screenshot(path=os.path.join(CIKTI, "icra4_dar.png"))
    b.close()

assert not hatalar, hatalar
print("TAMAM — 16 adım geçti (Tebligatlar + Tür eşleştir dahil) — (çoklu seçim, Tümünü seç / hariç, ayrı daire–dosya no, UYAP'tan güncelleme, takip türü, kesinleşme süreleri dahil);", len(istekler), "liste isteği")
