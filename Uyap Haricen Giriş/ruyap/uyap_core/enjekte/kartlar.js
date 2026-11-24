// ═══════════════════════════════════════════════════════════════════════════════════════
// ÇOKLU UYAP — UYAP DOSYA KARTLARI GÖREV ÇUBUĞUNDA (2026-11-11)
// ═══════════════════════════════════════════════════════════════════════════════════════
// UYAP'ın KENDİ dosya kartı korunur (görünüm ve karta bağlı bütün eklentiler: sağ tık, iş atama,
// indirme, 121, barkod…). Kart başlığına UYAP'ın kırmızı X'inin soluna "–" (Küçült) eklenir:
// basınca kartın o anki sekmesi ve seçili belgesi saklanır, kart kapanır ve alttaki görev
// çubuğunda (gorevcubugu.js, CU.gorev) bir düğme olarak kalır. Düğmeye tıklayınca aynı kart o sekme
// ve o belgeyle yeniden açılır; ekranda başka kart açıksa o kart görev çubuğuna geçer.
// UYAP aynı anda tek kart gösterebildiği için (bkz. uyapKartiAc) ekranda hep tek kart vardır.
//
// Açık kart da görev çubuğunda vurgulu durur (tıklayınca küçülür). X ile kapatılan kart
// listeden çıkar; sayfa değişimi gibi başka bir nedenle kaybolan kart küçültülmüş sayılır.
// Liste localStorage'da saklanır (sayfa yenilense de kalır).
//
// Açma: UYAP'ın ana sayfadaki arama bileşeninin kart kancası (__uyapKartiAcDogrudan). Başka
// sayfadaysak sayfa YENİLENMEDEN ana sayfaya geçilir (history.pushState + popstate; canlı
// denendi). dosyaId oturuma bağlı: bu sayfa yüklemesinde görülen kimlik doğrudan kullanılır,
// yoksa Dosya Sorgulama ucuyla tazelenir (__uyapTazeKart, önbelleği atlayan ham istek).
// Oturumda daha önce açılmış kartın verisi enjekte betiğin önbelleğinden (SWR) anında gelir.
//
// Seçili belge: evrak ağacında aria-selected="true" düğüm; kök klasör (ör. "1_2" = Son 20 Evrak) ile
// ggEvrakId (aynı yüklemede kesin eşleşme; her yüklemede yeniden üretilir) ve kalıcı birimEvrakNo + tür
// (ek belgede ana belgenin no'su + ekin adı) saklanır; geri açarken bu sırayla aranır.
//
// Telefon (mobil.js, mobil.css): kart hep tam ekran, başlık düğmeleri simge; Evrak sekmesi tek bölme
// (belge listesi ⇄ belge). Görev çubuğundaki düğme iki satır: dosya no / birim.
(function() {
    'use strict';
    const CU = window.CU;
    if (!CU || !CU.gorev || window.__uyapKartGorev) return;
    window.__uyapKartGorev = true;

    const DEPO = 'cokluuyapKartlar';
    const ENFAZLA = 30;
    let kartlar = [];                 // {anahtar, ad, kart, sekme, belge, t, acik}
    try { kartlar = (JSON.parse(localStorage.getItem(DEPO) || '[]') || []).filter(function(k) { return k && k.anahtar && k.kart; }); } catch (e) {}
    kartlar.forEach(function(k) { k.acik = false; });
    const oturumKimlik = {};          // anahtar → bu sayfa yüklemesinde geçerli dosyaId
    let mesgul = false, xIleKapanan = null;

    const bekle = function(ms) { return new Promise(function(r) { setTimeout(r, ms); }); };
    async function bekleKadar(kosul, ms) {
        for (let t = 0; t < ms; t += 150) { const v = kosul(); if (v) return v; await bekle(150); }
        return null;
    }
    function kaydet() { try { localStorage.setItem(DEPO, JSON.stringify(kartlar)); } catch (e) {} }
    function bildir(m) { if (window.__uyapBildirim) window.__uyapBildirim(m); }
    function bul(anahtar) { return kartlar.find(function(k) { return k.anahtar === anahtar; }) || null; }

    // ── Açık UYAP kartı ve durumu ──
    function gorunurKart() {
        const b = Array.from(document.querySelectorAll('.dosya-sorgula-popup .dx-popup-title'))
            .find(function(x) { return x.getClientRects().length > 0; });
        return b ? b.closest('.dosya-sorgula-popup') : null;
    }
    function kisaBirim(ad) {
        return String(ad || '').replace(/\bDairesi\b/i, '').replace(/\bMahkemesi\b/i, 'Mah.').replace(/\s+/g, ' ').trim();
    }
    function kisaAd(d) {
        return [kisaBirim(d.birimAdi), d.dosyaNo].filter(Boolean).join(' ');
    }
    function seciliBelge(kartEl) {
        try {
            const tv = kartEl.querySelector('.evrak-treeview');
            const n = tv && tv.querySelector('.dx-treeview-node[aria-selected="true"]');
            const A = window.__uyapEvrakAgaci;
            if (!n || !A) return null;
            const g = A.harita(A.veri(tv))[n.getAttribute('data-item-id')];
            const e = g && g.x && g.x.evrak;
            if (!e || !e.evrakId) return null;
            // Ek belgede (isEkEvrak) birim evrak no olmayabilir: ana belgenin no'su + ekin adı da saklanır.
            const ust = g.x.isEkEvrak && g.ebeveyn && g.ebeveyn.evrak ? g.ebeveyn.evrak : null;
            return { kok: n.getAttribute('data-item-id').split('_').slice(0, 2).join('_'),
                     gg: e.ggEvrakId || null, bno: e.birimEvrakNo || null, tur: e.tur || '',
                     ad: g.x.text || '', ustBno: ust ? (ust.birimEvrakNo || null) : null };
        } catch (e) { return null; }
    }
    function kartDurumu(kartEl) {
        const d = window.__uyapKartVerisi && window.__uyapKartVerisi(kartEl);
        if (!d || !d.dosyaNo || !d.birimId) return null;
        const kart = window.__uyapKartAlanlari ? window.__uyapKartAlanlari(d) : Object.assign({}, d);
        // Dosya Sorgulama sonucundan açılan kartta tür açıklaması "dosyaTur" adıyla gelebiliyor;
        // eksik kalırsa UYAP kart başlığında "…-undefined" yazıyor.
        if (!kart.dosyaTurKodAciklama && (d.dosyaTur || d.dosyaTurKodAciklama)) kart.dosyaTurKodAciklama = d.dosyaTur || d.dosyaTurKodAciklama;
        return { anahtar: window.__uyapDosyaAnahtari(d), ad: kisaAd(d), kart: kart, dosyaId: d.dosyaId,
                 sekme: window.__uyapSeciliSekme ? window.__uyapSeciliSekme(kartEl) : '', belge: seciliBelge(kartEl) };
    }

    // Kullanıcı kartı kendisi küçülttü ya da X ile kapattı: dinleyenlere haber ver (icra.js: kart İcra Takip
    // Süreçleri'nden açıldıysa o sayfa yeniden öne gelir). Başka karta geçerken yapılan küçültme bildirilmez.
    const kapanisDinleyicileri = [];
    function kapanisBildir(anahtar, neden) {
        kapanisDinleyicileri.forEach(function(f) { try { f(anahtar, neden); } catch (e) {} });
    }
    window.__cuKartKapaninca = function(f) { kapanisDinleyicileri.push(f); };
    window.__cuGorunurKartAnahtari = function() { const el = gorunurKart(); const s = el && kartDurumu(el); return s ? s.anahtar : null; };
    window.__cuGorunurKartiKucult = function() { const el = gorunurKart(); return el ? kullaniciKucult(el) : false; };
    // Başka ekrandan (İş Listesi "İş ata" sonrası) kartı yeniden açma: sekmesi ve belgesiyle.
    window.__cuKartAc = function(anahtar) { if (bul(anahtar)) ac(anahtar); };
    function kullaniciKucult(kartEl) {
        const s = kartDurumu(kartEl);
        const ok = kucult(kartEl);
        if (ok && s) kapanisBildir(s.anahtar, 'kucult');
        return ok;
    }

    // ── Küçült: durumu sakla, kartı kapat ──
    function kucult(kartEl) {
        const s = kartDurumu(kartEl);
        if (!s) return false;
        let k = bul(s.anahtar);
        if (!k) { k = { anahtar: s.anahtar }; kartlar.push(k); }
        Object.assign(k, { ad: s.ad, kart: s.kart, sekme: s.sekme, belge: s.belge, t: Date.now(), acik: false });
        if (s.dosyaId) oturumKimlik[s.anahtar] = s.dosyaId;
        while (kartlar.length > ENFAZLA) kartlar.shift();
        kaydet();
        if (window.__uyapUyapKartiniKapat) window.__uyapUyapKartiniKapat(kartEl);
        ciz();
        return true;
    }

    // ── Aç: (gerekirse açık kartı küçült) → ana sayfa → kartı aç → sekme + belge ──
    async function ac(anahtar) {
        const k = bul(anahtar);
        if (!k || mesgul) return;
        mesgul = true;
        ciz();
        try {
            const acik = gorunurKart();
            if (acik) {
                const s = kartDurumu(acik);
                if (s && s.anahtar === anahtar) return;                  // zaten açık
                kucult(acik);
                await bekleKadar(function() { return !gorunurKart(); }, 3000);
            }
            if (!document.getElementById('searchId')) {
                history.pushState({}, '', '/');
                window.dispatchEvent(new PopStateEvent('popstate', { state: {} }));
                if (!await bekleKadar(function() { return document.getElementById('searchId'); }, 8000)) {
                    bildir('UYAP ana sayfası açılamadı; kartı ana sayfadan yeniden deneyin.');
                    return;
                }
            }
            let kart = Object.assign({}, k.kart);
            if (oturumKimlik[anahtar]) kart.dosyaId = oturumKimlik[anahtar];
            else {
                const t = window.__uyapTazeKart ? await window.__uyapTazeKart(kart) : null;
                if (!t || !t.dosyaId) { bildir('Dosya UYAP\'ta bulunamadı: ' + k.ad); return; }
                kart = t;
                oturumKimlik[anahtar] = t.dosyaId;
            }
            if (!window.__uyapKartiAcDogrudan || !window.__uyapKartiAcDogrudan(kart)) {
                bildir('Dosya kartı açılamadı (UYAP sayfası değişmiş olabilir).');
                return;
            }
            const kartEl = await bekleKadar(function() {
                const el = gorunurKart();
                const s = el && kartDurumu(el);
                return s && s.anahtar === anahtar ? el : null;
            }, 12000);
            if (!kartEl) return;
            if (k.sekme && window.__uyapSekmeSec && window.__uyapSeciliSekme(kartEl) !== k.sekme) {
                // Sekmeler kart başlığından SONRA çizilir (canlı, 2026-11-11: başlık görünür görünmez tek kez
                // denenince sekme henüz yoktu, kart ilk sekmede kalıyor, belge de geri gelmiyordu): sekme
                // seçilene kadar yeniden denenir.
                await bekleKadar(function() {
                    window.__uyapSekmeSec(kartEl, k.sekme);
                    return window.__uyapSeciliSekme(kartEl) === k.sekme;
                }, 6000);
                await bekle(300);
            }
            if (k.sekme === 'Evrak' && k.belge) await belgeSec(kartEl, k.belge);
        } catch (e) {
            bildir('Kart açılamadı: ' + (e && e.message || e));
        } finally {
            mesgul = false;
            izle();
        }
    }

    async function belgeSec(kartEl, b) {
        const A = window.__uyapEvrakAgaci;
        const tv = await bekleKadar(function() { return kartEl.querySelector('.evrak-treeview'); }, 6000);
        if (!tv || !A) return;
        const kok = await bekleKadar(function() { return tv.querySelector('.dx-treeview-node[data-item-id="' + b.kok + '"]'); }, 6000);
        if (!kok) return;
        const ac_ = function(n) {
            if (n.getAttribute('aria-expanded') === 'false') {
                const t = n.querySelector(':scope > .dx-treeview-toggle-item-visibility') || n.querySelector(':scope > .dx-treeview-item');
                if (t) t.click();
            }
        };
        ac_(kok);
        // Ağaç verisinde belgeyi bul, atalarını aç, tıkla. Önce ggEvrakId; o her yüklemede yeniden üretildiği için
        // (canlı, 2026-11-11: sayfa yenilenince aynı belgenin ggEvrakId'si farklıydı, belge geri gelmiyordu)
        // tutmazsa birim evrak no + tür, ek belgede ana belgenin no'su + ekin adı.
        const hedef = await bekleKadar(function() {
            const h = A.harita(A.veri(tv));
            const adaylar = Object.keys(h).filter(function(id) { return id.indexOf(b.kok + '_') === 0 && h[id].x && h[id].x.evrak; });
            const bul = function(kosul) { return adaylar.find(function(id) { return kosul(h[id].x.evrak, h[id].x, h[id].ebeveyn); }) || null; };
            return (b.gg && bul(function(e) { return e.ggEvrakId === b.gg; }))
                || (b.bno != null && bul(function(e, x) { return !x.isEkEvrak && e.birimEvrakNo === b.bno && (e.tur || '') === (b.tur || ''); }))
                || (b.ustBno != null && b.ad && bul(function(e, x, ust) {
                    return !!x.isEkEvrak && !!ust && !!ust.evrak && ust.evrak.birimEvrakNo === b.ustBno && (x.text || '') === b.ad;
                }))
                || null;
        }, 4000);
        if (!hedef) return;
        const parca = hedef.split('_');
        for (let i = 3; i < parca.length; i++) {           // kök ile belge arasındaki klasörler
            const ata = tv.querySelector('.dx-treeview-node[data-item-id="' + parca.slice(0, i).join('_') + '"]');
            if (ata) { ac_(ata); await bekle(250); }
        }
        const n = await bekleKadar(function() { return tv.querySelector('.dx-treeview-node[data-item-id="' + hedef + '"]'); }, 3000);
        const satir = n && n.querySelector(':scope > .dx-treeview-item');
        if (!satir) return;
        satir.click();
        satir.scrollIntoView({ block: 'center' });
    }

    // ── Kart başlığında Windows'taki gibi üç düğme: küçült · tam ekran / önceki boyut · kapat ──
    // (kullanıcı kararı 2026-11-11). Kapat UYAP'ın kırmızı X'i; UYAP'ın yazılı "Tam Ekran" düğmesi gizlenir,
    // bizim simgeli düğmemiz onu tetikler (UYAP'ın kendi tam ekranı çalışır, içerik yerleşimi bozulmaz).
    const UYAP_TAM = /Tam Ekran|Normal Ekran|Küçült/i;
    function uyapTamDugmesi(bas) {
        return Array.from(bas.querySelectorAll('[role=button]')).find(function(b) {
            return !b.hasAttribute('data-cu-tam') && !b.hasAttribute('data-kucult')
                && (UYAP_TAM.test(b.textContent) || b.querySelector('.icon-maximize, .icon-minimize'));
        }) || null;
    }
    function tamEkranMi() {
        const c = document.querySelector('.dx-overlay-wrapper.dosya-sorgula-popup > .dx-overlay-content');
        return !!(c && c.classList.contains('dx-popup-fullscreen'));
    }
    function pencereDugmesi(ozellik, ikon, ipucu, fn) {
        const b = document.createElement('div');
        b.setAttribute(ozellik, '1');
        b.setAttribute('role', 'button');
        b.setAttribute('tabindex', '0');
        b.title = ipucu;
        b.className = 'dx-widget dx-button dx-button-mode-outlined dx-button-normal dx-button-has-icon adalet-button cu-pd';
        b.innerHTML = '<div class="dx-button-content"><span class="dx-icon fe ' + ikon + '"></span></div>';
        b.addEventListener('click', function(ev) { ev.stopPropagation(); fn(); });
        return b;
    }
    function pencereDugmeleri(kartEl) {
        const bas = kartEl.querySelector('.dx-popup-title');
        if (!bas) return;
        const x = Array.from(bas.querySelectorAll('[role=button]')).find(function(b) {
            return !b.textContent.trim() && b.querySelector('.icon-x, .dx-icon-close');
        });
        if (!x || !x.parentElement) return;
        const uyapTam = uyapTamDugmesi(bas);
        if (uyapTam && uyapTam.style.display !== 'none') uyapTam.style.display = 'none';
        if (!x.hasAttribute('data-x-izli')) {
            x.setAttribute('data-x-izli', '1');
            // X ile kapatılan kart görev çubuğundan da çıkar.
            x.addEventListener('click', function() { const s = kartDurumu(kartEl); xIleKapanan = s ? s.anahtar : null; }, true);
        }
        let kucuk = bas.querySelector('[data-kucult]');
        if (!kucuk) {
            kucuk = pencereDugmesi('data-kucult', 'icon-minus', 'Görev çubuğuna küçült (sekme ve seçili belge hatırlanır)', function() { kullaniciKucult(kartEl); });
            x.parentElement.insertBefore(kucuk, x);
        }
        let tam = bas.querySelector('[data-cu-tam]');
        if (!tam && uyapTam) {
            tam = pencereDugmesi('data-cu-tam', 'icon-maximize', 'Tam ekran', function() {
                const u = uyapTamDugmesi(bas);
                if (u) u.click();
            });
            x.parentElement.insertBefore(tam, x);
        }
        if (tam) {
            const tamda = tamEkranMi();
            const ikon = tam.querySelector('.dx-icon');
            const sinif = 'dx-icon fe ' + (tamda ? 'icon-minimize' : 'icon-maximize');
            if (ikon && ikon.className !== sinif) ikon.className = sinif;
            tam.title = tamda ? 'Önceki boyut' : 'Tam ekran';
        }
        // Sıra: – □ × (React başlığı yeniden çizse de korunur)
        if (kucuk.nextElementSibling !== (tam || x)) x.parentElement.insertBefore(kucuk, tam || x);
        if (tam && tam.nextElementSibling !== x) x.parentElement.insertBefore(tam, x);
    }

    // Bir öğenin ait olduğu DevExtreme bileşeni (devextreme-react sınıfının instance'ı), React ağacından;
    // kosul(w) doğru dönen ilk bileşen.
    function dxBileseni(e, kosul) {
        for (; e && e.nodeType === 1; e = e.parentElement) {
            const a = Object.keys(e).find(function(k) { return k.indexOf('__reactFiber') === 0; });
            if (!a) continue;
            for (let f = e[a], i = 0; f && i < 40; i++, f = f.return) {
                const sn = f.stateNode;
                if (!sn || sn.nodeType || !('instance' in sn)) continue;
                try {
                    const w = typeof sn.instance === 'function' ? sn.instance() : sn.instance;
                    if (w && typeof w.option === 'function' && kosul(w)) return w;
                } catch (x) {}
            }
            return null;
        }
        return null;
    }
    // Animasyonlar kapatılır (kullanıcı: göz yoruyor): kartın açılış / kapanışı (Popup 'animation') ve
    // sekme geçişleri (TabPanel 'animationEnabled' / kaydırma). Her turda denetlenir: UYAP bileşeni
    // yeniden çizerse yeniden kapatılır.
    function animasyonlariKapat(kartEl) {
        const sar = document.querySelector('.dx-overlay-wrapper.dosya-sorgula-popup');
        if (sar && !sar.hasAttribute('data-cu-anim')) {
            const p = dxBileseni(kartEl, function(w) { return w.option('fullScreen') !== undefined; });
            if (p) { p.option('animation', null); sar.setAttribute('data-cu-anim', '1'); }
        }
        const tp = kartEl.querySelector('.dx-tabpanel');
        const t = tp && dxBileseni(tp, function(w) { return w.option('animationEnabled') !== undefined && w.option('swipeEnabled') !== undefined; });
        if (t && (t.option('animationEnabled') || t.option('swipeEnabled'))) {
            t.option('animationEnabled', false);
            t.option('swipeEnabled', false);
        }
    }
    // Kart her açılışta tam ekran gelir (kullanıcı kararı 2026-11-11). Kullanıcı "önceki boyut"a alırsa o
    // açılış boyunca öyle kalır; kart kapanıp yeniden açılınca yine tam ekran.
    let tamEkranlanan = null;
    function tamEkranAc(kartEl, anahtar) {
        if (tamEkranlanan === anahtar) return;
        const bas = kartEl.querySelector('.dx-popup-title');
        const u = bas && uyapTamDugmesi(bas);
        if (!u) return;                                   // başlık henüz çizilmedi: sonraki turda
        tamEkranlanan = anahtar;
        if (!tamEkranMi()) u.click();
    }

    // ── Açık kartı izle: görev çubuğunda vurgula; X ile kapanınca çıkar ──
    function izle() {
        const el = gorunurKart();
        if (el) { pencereDugmeleri(el); animasyonlariKapat(el); }
        const s = el && kartDurumu(el);
        if (s) tamEkranAc(el, s.anahtar); else tamEkranlanan = null;
        let degisti = false;
        kartlar.forEach(function(k) {
            const acik = !!s && k.anahtar === s.anahtar;
            if (k.acik && !acik) {
                if (xIleKapanan === k.anahtar) { k.sil = true; xIleKapanan = null; kapanisBildir(k.anahtar, 'x'); }
                else k.t = Date.now();                     // başka nedenle kayboldu: küçültülmüş say
            }
            if (k.acik !== acik) { k.acik = acik; degisti = true; }
        });
        if (kartlar.some(function(k) { return k.sil; })) { kartlar = kartlar.filter(function(k) { return !k.sil; }); degisti = true; }
        if (s) {
            let k = bul(s.anahtar);
            if (!k) { k = { anahtar: s.anahtar, acik: true }; kartlar.push(k); degisti = true; }
            if (k.ad !== s.ad) { k.ad = s.ad; degisti = true; }
            k.kart = s.kart;
            if (s.dosyaId) oturumKimlik[s.anahtar] = s.dosyaId;
        }
        if (degisti) kaydet();
        ciz();
    }

    function ciz() {
        CU.gorev.ayarla(kartlar.map(function(k) {
            // Telefonda düğme iki satır (gorevcubugu.js, mobil.css): üstte dosya no, altta kısa birim adı; sona
            // sığmayan uzun adda en önemli bilgi (dosya no) kesiliyordu.
            const no = k.kart && k.kart.dosyaNo ? String(k.kart.dosyaNo).trim() : '';
            return {
                kimlik: 'kart:' + k.anahtar, ad: k.ad || k.anahtar, ikon: '📁', on: !!k.acik, kucuk: !k.acik,
                ust: no, alt: no ? kisaBirim(k.kart.birimAdi) : '',
                tikla: function() {
                    if (k.acik) { const el = gorunurKart(); if (el) kullaniciKucult(el); }
                    else ac(k.anahtar);
                },
                kapat: function() {
                    if (k.acik) { const el = gorunurKart(); if (el && window.__uyapUyapKartiniKapat) { xIleKapanan = k.anahtar; window.__uyapUyapKartiniKapat(el); } }
                    kartlar = kartlar.filter(function(x) { return x !== k; });
                    kaydet();
                    ciz();
                },
                menu: function() { return [[[k.acik ? 'Küçült' : 'Aç', function() { if (k.acik) { const el = gorunurKart(); if (el) kullaniciKucult(el); } else ac(k.anahtar); }]]]; },
            };
        }), 'kart');
    }

    // Kart görev çubuğuna yer bıraksın: UYAP kartı tüm yüksekliği kaplayınca (küçük ekran) görev
    // çubuğu örtünün altında kalıp tıklanamıyordu. Kart alanı görev çubuğu kadar kısaltılır
    // (--cu-cubuk-yuk, gorevcubugu.js); DevExtreme kartı kalan alanda ortalar (canlı denendi, 2026-11-11).
    const st = document.createElement('style');
    st.textContent = '.dx-overlay-wrapper.dosya-sorgula-popup{height:calc(100% - var(--cu-cubuk-yuk, 44px))!important}' +
        '.dx-overlay-wrapper.dosya-sorgula-popup>.dx-overlay-content{max-height:calc(100vh - 2 * var(--cu-cubuk-yuk, 44px))!important;transition:none!important}' +
        // Tam ekran: görev çubuğuna kadar (eskiden üstte bırakılan pay yüzünden altta boşluk kalıyordu).
        '.dx-overlay-wrapper.dosya-sorgula-popup>.dx-overlay-content.dx-popup-fullscreen{max-height:none!important;height:100%!important;top:0!important}' +
        // Pencere düğmeleri: kare, simgeli
        '.dosya-sorgula-popup .dx-popup-title .cu-pd{margin-right:6px;min-width:34px}' +
        // Koyu tema (UYAP: html[data-theme=dark]): kart tablolarındaki Bootstrap çizgili satırlar açık gri zemin +
        // siyah yazıyla kalıyordu (ör. Dosya Bilgileri, td bg rgb(241,241,241)). Zemin şeffaf, yazı kartın rengi,
        // çizgi hafif açık ton.
        'html[data-theme="dark"] .dosya-sorgula-popup table.table > :not(caption) > * > *{background-color:transparent!important;color:inherit!important;box-shadow:none!important}' +
        'html[data-theme="dark"] .dosya-sorgula-popup table.table-striped > tbody > tr:nth-of-type(odd) > *{background-color:rgba(255,255,255,0.05)!important}';
    (document.head || document.documentElement).appendChild(st);

    // ── Sol menü açıkken kart menünün sağındaki alana yerleşir (Armonia Yazı yan yana açıkken de onun solundaki alana) (kullanıcı bulgusu 2026-11-11: yarım menü
    // kadar kaydırmak geniş kartta yetmiyordu). DevExtreme kartı pencereye göre ortalayıp transform ile
    // konumluyor; ona dokunmadan bağımsız CSS 'translate' ile sağa kaydırılır, genişlik kalan alana sığdırılır.
    // Normal: kartın solu menünün en az 12 px sağında, kalan alanda ortalı. Tam ekran: menünün bittiği yerden
    // ekranın sağına kadar. Yalnız DevExtreme'in verdiği yer iki ölçümde aynıysa (oturmuşsa) kaydırılır.
    let oncekiTaban = null;
    function kartiHizala() {
        const c = document.querySelector('.dx-overlay-wrapper.dosya-sorgula-popup > .dx-overlay-content');
        if (!c) { oncekiTaban = null; return; }
        const yan = document.getElementById('av-sidebar-wrapper');
        // Dar ekranda (mobil.js) menü sayfayı itmez, kartın üstüne açılır: kart yerinde kalır.
        const acik = document.body.classList.contains('cu-solacik') && yan && !(window.__cuDar && window.__cuDar());
        // Armonia Yazı sağda yan yana açıksa (enjekte/yazi.js) kart soldaki alana sığar: belge okunurken yanda
        // yazılır (kullanıcı bulgusu 23.11.2026: kart editörün altında kalıyordu).
        const sagPay = window.__cuYaziSagPay ? window.__cuYaziSagPay() : 0;
        if (!acik && !sagPay) {
            if (c.style.translate || c.style.maxWidth) { c.style.translate = ''; c.style.maxWidth = ''; }
            oncekiTaban = null;
            return;
        }
        const tam = c.classList.contains('dx-popup-fullscreen');
        const sol = acik ? yan.offsetWidth : 0;
        const alanGen = document.documentElement.clientWidth - sol - sagPay;
        const mg = (tam ? alanGen : alanGen - 24) + 'px';
        if (c.style.maxWidth !== mg) c.style.maxWidth = mg;
        const r = c.getBoundingClientRect();
        if (!r.width) return;
        const mevcut = parseFloat(c.style.translate) || 0;
        const taban = Math.round(r.left - mevcut);           // DevExtreme'in verdiği yer
        const oturdu = oncekiTaban !== null && Math.abs(taban - oncekiTaban) <= 1;
        oncekiTaban = taban;
        if (!oturdu) return;
        const hedef = tam ? sol : sol + Math.max(12, (alanGen - r.width) / 2);
        const t = Math.round(hedef - taban);
        if (Math.abs(t - mevcut) > 1) c.style.translate = t + 'px 0';
    }
    setInterval(kartiHizala, 100);

    setInterval(izle, 300);
    ciz();
})();
