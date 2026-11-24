// ═══════════════════════════════════════════════════════════════════════════════════════
// ÇOKLU UYAP — İCRA TAKİP SÜREÇLERİ SAYFASI (2026-11-15)
// ═══════════════════════════════════════════════════════════════════════════════════════
// Ana menüye (sol menü, #av-sidebar-wrapper) "İcra Takip Süreçleri" girişi eklenir; basınca içerik alanının
// üstünde bizim sayfamız açılır (görev çubuğu ve menü yerinde kalır).
//
// Pencere gibi davranır (kullanıcı isteği 16.11.2026: "iş akışı akıp gitsin, kayıp gitmesin"):
// • Görev çubuğunda kendi düğmesi var (CU.gorev, kaynak 'icra'): tıklayınca küçülür / öne gelir. Küçültülünce
//   sayfa silinmez, gizlenir: filtre, sayfa, seçili dosya paneli ve kaydırma yerinde kalır.
// • Menüde başka bir sayfaya gidilince sayfa kapanmaz, görev çubuğuna küçülür. Yalnız "Kapat" kapatır.
// • "UYAP'ta aç": dosya kartı bu sayfanın ÜSTÜNDE açılır (kart z ≤ 1590, sayfa 1400). Kart küçültülünce ya da
//   X ile kapatılınca kullanıcı kaldığı yere, bu sayfaya döner. Bu arada sayfa küçülmüşse de
//   (kartlar.js __cuKartKapaninca) yeniden öne gelir.
// Açık / küçük hâli sayfa yenilemesinden sonra da korunur (sessionStorage: '1' açık, 'k' küçük).
//
// Veri: ofis bilgisayarındaki büro veritabanı, /__cu__/veri/… uçları (src/icra_api.rs). Uçlar bütün büro
// kullanıcılarına açık (kullanıcı kararı 15.11.2026): bu bilgisayarda vekil, uzakta tünel cevaplar.
// UYAP'a istek atılmaz; yalnız "UYAP'ta aç" düğmesi dosyayı UYAP'ta bulup kendi kartını açar.
//
// Sekmeler: Dosya Listesi (dosya başına satır, 20.11.2026) · Süreçler (aşama şeridi + borçlu listesi + dosya paneli) ·
// Tebligatlar · Kesinleşme Süreleri · Resmî Tatiller.
//
// Liste (kullanıcı isteği 16.11.2026): her borçlunun BÜTÜN tebligatları (barkodları) satırında görünür. Kesinleşme,
// tebligat türü, tebliğ durumu ve mazbata başlıklarında "yaz ya da seç" kutusu var (UyapMasa'daki başlık kutuları):
// yazınca içinde geçenleri, listeden seçince tam o değeri süzer. Seçenekler adetleriyle sunucudan gelir ve diğer
// süzgeçlere göre daralır (boş sonuç veren seçenek görünmez). Tür + tebliğ + mazbata aynı tebligatta birlikte aranır.
// Seçili süzgeçler listenin üstünde etiket olarak durur; tek tıkla kalkar.
// Renkler tema değişkenlerinden (--cu-t-*, --cu-vurgu*); UYAP / Koyu temalarında kendi yedek değerleri.
(function() {
    'use strict';
    const anaCerceve = window.__uyapAnaCerceve ? window.__uyapAnaCerceve() : window.top === window;
    if (!anaCerceve || window.__cuIcraTakip) return;
    window.__cuIcraTakip = true;

    const UC = '/__cu__/veri/';
    const ACIK_ANAHTAR = 'cuIcraTakipAcik';
    const ASAMA_RENK = {
        takip_talebi: '#8a877e', odeme_emri: '#8a877e', teb_olusturuluyor: '#3f7fa3', ptt_veriliyor: '#3f7fa3',
        teblige_cikti: '#3f7fa3', teblig: '#b8901f', iade: '#c06a2f', itiraz: '#b84444', durduruldu: '#7a5fb0',
        kesinlesti: '#4f8a57', '': '#8a877e'
    };
    const ASAMA_KISA = {
        takip_talebi: 'Takip Talebi', odeme_emri: 'Ödeme Emri', teb_olusturuluyor: 'Tebligat Oluşturuluyor',
        ptt_veriliyor: "PTT'ye Veriliyor", teblige_cikti: 'Tebliğe Çıktı', teblig: 'Tebliğ', iade: 'İade',
        itiraz: 'İtiraz', durduruldu: 'Durduruldu', kesinlesti: 'Kesinleşti', '': 'Hesaplanmadı'
    };
    const SIRA = ['takip_talebi', 'odeme_emri', 'teb_olusturuluyor', 'ptt_veriliyor', 'teblige_cikti', 'teblig', 'kesinlesti'];

    // ── Yardımcılar ──
    function e(tag, oz, cocuklar) {
        const x = document.createElement(tag);
        if (oz) Object.keys(oz).forEach(function(k) {
            const v = oz[k];
            if (v === undefined || v === null || v === false) return;
            if (k === 'sinif') x.className = v;
            else if (k === 'metin') x.textContent = v;
            else if (k === 'stil') x.setAttribute('style', v);
            else if (k.indexOf('on') === 0) x.addEventListener(k.slice(2), v);
            else x.setAttribute(k, v === true ? '' : v);
        });
        (cocuklar || []).forEach(function(c) {
            if (c === null || c === undefined || c === false) return;
            x.appendChild(typeof c === 'string' ? document.createTextNode(c) : c);
        });
        return x;
    }
    function bos(x) { while (x && x.firstChild) x.removeChild(x.firstChild); return x; }
    function gunMetni(iso) {
        const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(iso || '');
        return m ? m[3] + '.' + m[2] + '.' + m[1] : '';
    }
    function bildir(m) { if (window.__uyapBildirim) window.__uyapBildirim(m); }
    const dar = function() { return !!(window.__cuDar && window.__cuDar()); };

    async function istek(uc, govde) {
        let r;
        try {
            r = await fetch(UC + uc, govde === undefined
                ? { cache: 'no-store', credentials: 'same-origin' }
                : { method: 'POST', cache: 'no-store', credentials: 'same-origin',
                    headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(govde) });
        } catch (x) {
            throw new Error('Ofis bilgisayarına ulaşılamadı.');
        }
        let j = null;
        try { j = await r.json(); } catch (x) {}
        if (!j) throw new Error('Ofis bilgisayarından beklenmeyen yanıt (' + r.status + '). Program güncel olmayabilir.');
        if (!j.ok) {
            if (j.kod === 'E-402') throw new Error('Büro veritabanı hazır değil (E-402). Ofis bilgisayarında program açık ve paylaşım başlamış olmalı.');
            throw new Error(j.error || 'İşlem yapılamadı.');
        }
        return j.veri;
    }

    // ── Stil ──
    function stilKur() {
        if (document.getElementById('cu-icra-stil')) return;
        const css = [
            '#cu-icra{--i-zemin:var(--cu-t-zemin,#f3f4f7);--i-yuzey:var(--cu-t-yuzey,#fff);--i-serit:var(--cu-t-serit,#f5f6fa);',
            '--i-kenar:var(--cu-t-kenar,#dee2e6);--i-yazi:var(--cu-t-yazi,#212529);--i-vurgu:var(--cu-vurgu,#2d4596);',
            '--i-vurgu-koyu:var(--cu-vurgu-koyu,#1f3277);--i-vurgu-zemin:var(--cu-vurgu-zemin,#e8ecf8);--i-vurgu-yazi:var(--cu-vurgu-yazi,#1f2f70);',
            '--i-soluk:color-mix(in srgb,var(--i-yazi) 74%,var(--i-yuzey));--i-silik:color-mix(in srgb,var(--i-yazi) 64%,var(--i-yuzey));',
            'position:fixed;z-index:1400;top:var(--cu-nav-yuk,60px);right:0;bottom:var(--cu-cubuk-yuk,44px);left:0;',
            'background:var(--i-zemin);color:var(--i-yazi);font:14px/1.45 "Segoe UI",system-ui,sans-serif;display:flex;flex-direction:column;overflow:hidden}',
            // Güneş: vurgu sarı; vurgu rengindeki yazılar (seçili sekme, etiketler, bağlantılar) beyaz zeminde okunmuyordu
            // (2026-11-23 karşıtlık taraması). Yazı olarak temanın koyu vurgu-yazı tonu kullanılır.
            'html[data-cu-tema="gunes"] #cu-icra{--i-vurgu-koyu:var(--cu-vurgu-yazi,#6b4a00)}',
            'html[data-cu-tema="koyu"] #cu-icra{--i-zemin:#1d2130;--i-yuzey:#2a2f40;--i-serit:#262b3b;--i-kenar:#3a4055;--i-yazi:#dedede;',
            '--i-vurgu:#3a55b5;--i-vurgu-koyu:#8ea2e6;--i-vurgu-zemin:#2c3554;--i-vurgu-yazi:#c3cdf2}',
            '#cu-icra *{box-sizing:border-box}',
            '#cu-icra button,#cu-icra input,#cu-icra select{font:inherit;color:inherit}',
            '#cu-icra .i-bas{display:flex;align-items:center;gap:14px;padding:14px 22px 0;flex-wrap:wrap}',
            '#cu-icra .i-bas h1{font-size:19px;margin:0;font-weight:600}',
            '#cu-icra .i-durum{color:var(--i-soluk);font-size:12.5px}',
            '#cu-icra .i-bosluk{flex:1}',
            '#cu-icra .i-sekmeler{display:flex;gap:2px;padding:10px 22px 0;border-bottom:1px solid var(--i-kenar)}',
            '#cu-icra .i-sekmeler button{background:none;border:0;border-bottom:2px solid transparent;padding:8px 12px;color:var(--i-soluk);cursor:pointer;margin-bottom:-1px}',
            '#cu-icra .i-sekmeler button.on{color:var(--i-vurgu-koyu);border-color:var(--i-vurgu);font-weight:600}',
            '#cu-icra .i-govde{flex:1;display:flex;min-height:0;position:relative}',
            '#cu-icra .i-ana{flex:1;overflow:auto;padding:16px 22px 22px;min-width:0}',
            '#cu-icra .i-asamalar{display:flex;gap:8px;margin-bottom:14px;overflow-x:auto;padding-bottom:2px}',
            '#cu-icra .i-asama{flex:1 0 96px;background:var(--i-yuzey);border:1px solid var(--i-kenar);border-radius:12px;padding:9px 11px;text-align:left;cursor:pointer;min-width:96px}',
            '#cu-icra .i-asama .n{font-size:21px;font-weight:600;line-height:1.1}',
            '#cu-icra .i-asama .t{font-size:12px;color:var(--i-soluk);margin-top:2px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}',
            '#cu-icra .i-asama i{display:block;height:3px;border-radius:2px;margin-top:7px}',
            '#cu-icra .i-asama.on{border-color:var(--i-vurgu);box-shadow:0 0 0 2px var(--i-vurgu-zemin)}',
            '#cu-icra .i-arac{display:flex;gap:10px;align-items:center;margin-bottom:12px;flex-wrap:wrap}',
            '#cu-icra input[type=search],#cu-icra input[type=text],#cu-icra input[type=date],#cu-icra input[type=number],#cu-icra select{background:var(--i-yuzey);border:1px solid var(--i-kenar);border-radius:9px;padding:7px 10px;min-width:0}',
            '#cu-icra input[type=search]{flex:1;max-width:420px}',
            '#cu-icra .i-onay{display:flex;align-items:center;gap:6px;color:var(--i-soluk);cursor:pointer;user-select:none}',
            '#cu-icra .i-d{border:1px solid var(--i-kenar);background:var(--i-yuzey);border-radius:9px;padding:7px 13px;cursor:pointer;white-space:nowrap}',
            '#cu-icra .i-d:hover{border-color:var(--i-vurgu)}',
            '#cu-icra .i-d.ana{background:var(--i-vurgu);border-color:var(--i-vurgu);color:var(--cu-vurgu-uzeri,#fff)}',
            '#cu-icra .i-d.kucuk{padding:4px 9px;font-size:12.5px;border-radius:8px;color:var(--i-soluk)}',
            // Küçük + dolu düğme (ör. "Veritabanını güncelle", "Kesinleşmeyi kaydet"): üstteki soluk yazı rengi dolu
            // vurgu zemininde okunmuyordu (kullanıcı bildirimi 2026-11-23); yazı vurgu üstü rengine döner.
            '#cu-icra .i-d.kucuk.ana{color:var(--cu-vurgu-uzeri,#fff)}',
            '#cu-icra .i-d.tehlike{color:#b84444}',
            '#cu-icra .i-d[disabled]{opacity:.55;cursor:default}',
            '#cu-icra table{width:100%;border-collapse:separate;border-spacing:0;background:var(--i-yuzey);border:1px solid var(--i-kenar);border-radius:12px;overflow:hidden}',
            '#cu-icra th{text-align:left;font-weight:600;font-size:12px;color:var(--i-soluk);background:var(--i-serit);padding:8px 12px;white-space:nowrap}',
            '#cu-icra td{padding:8px 12px;border-top:1px solid var(--i-kenar);vertical-align:top}',
            '#cu-icra tbody.tik{cursor:pointer}#cu-icra tbody.tik:hover td{background:var(--i-serit)}',
            '#cu-icra tbody.sec td{background:var(--i-vurgu-zemin)!important}',
            // Başlıktaki "yaz ya da seç" kutusu + açılır seçenek listesi
            '#cu-icra table.liste th{vertical-align:top}#cu-icra th.suz{min-width:150px}#cu-icra th.suz-dosya_no{min-width:112px}',
            '#cu-icra .i-kutu-s{position:relative;display:flex;margin-top:5px}',
            '#cu-icra .i-kutu-s input{flex:1;min-width:0;width:100%;font-weight:400;font-size:12.5px;padding:5px 44px 5px 8px;border-radius:8px;text-overflow:ellipsis}',
            '#cu-icra .i-kutu-s.dolu input{border-color:var(--i-vurgu);background:var(--i-vurgu-zemin);color:var(--i-vurgu-yazi);font-weight:600}',
            '#cu-icra .i-kutu-s button{position:absolute;top:1px;bottom:1px;width:20px;border:0;background:none;color:var(--i-silik);cursor:pointer;padding:0;font-size:11px}',
            '#cu-icra .i-kutu-s .ok{right:3px}#cu-icra .i-kutu-s .sil{right:22px;font-size:13px}',
            '#cu-icra .i-kutu-s button:hover{color:var(--i-yazi)}',
            '#cu-icra .i-acilir{position:fixed;z-index:5;background:var(--i-yuzey);border:1px solid var(--i-kenar);border-radius:10px;box-shadow:0 10px 28px rgba(0,0,0,.18);',
            'max-height:320px;overflow:auto;padding:4px;min-width:220px;max-width:min(460px,calc(100vw - 16px));font-size:13px}',
            '#cu-icra .i-acilir .sec{display:flex;justify-content:space-between;gap:14px;padding:6px 9px;border-radius:7px;cursor:pointer;align-items:baseline}',
            '#cu-icra .i-acilir .sec:hover,#cu-icra .i-acilir .sec.imlec{background:var(--i-vurgu-zemin)}',
            '#cu-icra .i-acilir .sec.on{font-weight:600;color:var(--i-vurgu-koyu)}',
            '#cu-icra .i-acilir .sec .kt{flex:none;width:15px;height:15px;border:1.5px solid var(--i-kenar);border-radius:4px;align-self:center;display:flex;align-items:center;justify-content:center;font-size:11px;line-height:1;color:#fff}',
            '#cu-icra .i-acilir .sec.on .kt{background:var(--i-vurgu-koyu);border-color:var(--i-vurgu-koyu)}',
            '#cu-icra .i-acilir .sec .ad{flex:1;min-width:0}',
            '#cu-icra .i-acilir .sec span:last-child{color:var(--i-silik);font-size:12px;white-space:nowrap;font-weight:400}',
            '#cu-icra .i-acilir .yok{padding:8px 9px;color:var(--i-silik)}',
            '#cu-icra .i-secili{display:flex;gap:6px;flex-wrap:wrap;align-items:center;margin:-2px 0 12px;font-size:12.5px;color:var(--i-soluk)}',
            '#cu-icra .i-secili .et{display:inline-flex;gap:6px;align-items:center;background:var(--i-vurgu-zemin);color:var(--i-vurgu-yazi);border:0;border-radius:999px;padding:3px 6px 3px 11px;cursor:pointer;max-width:100%}',
            '#cu-icra .i-secili .et b{font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:280px}',
            '#cu-icra .i-secili .et i{font-style:normal;opacity:.7;padding:0 4px}',
            '#cu-icra .i-suzgecler{display:flex;gap:8px;flex-wrap:wrap;margin-bottom:12px}',
            '#cu-icra .i-suzgecler .i-kutu-s{flex:1 1 150px;margin:0}',
            '#cu-icra .i-suzgecler.ust .i-kutu-s{flex:0 1 220px}',
            '#cu-icra .i-gt{background:var(--i-yuzey);border:1px solid var(--i-kenar);border-radius:12px;padding:12px 14px;margin-bottom:12px}',
            '#cu-icra .i-gt-ust{display:flex;align-items:center;gap:8px;margin-bottom:6px}',
            '#cu-icra .i-gt-form{display:flex;gap:10px;align-items:center;flex-wrap:wrap;margin-top:8px}',
            '#cu-icra .i-gt-form select{max-width:340px}',
            '#cu-icra .i-gt-cubuk{height:8px;border-radius:5px;background:var(--i-serit);overflow:hidden;margin:8px 0 4px;border:1px solid var(--i-kenar)}',
            '#cu-icra .i-gt-cubuk i{display:block;height:100%;background:var(--i-vurgu);transition:width .4s}',
            '#cu-icra .i-gt-not{color:var(--i-soluk);font-size:12.5px}',
            '#cu-icra .i-gt-uyari{color:#a15c00;font-size:12.5px;margin-top:6px}',
            'html[data-cu-tema="koyu"] #cu-icra .i-gt-uyari{color:#f0b35a}',
            '#cu-icra .i-gt-rapor{margin:6px 0 0;padding:0;list-style:none;font-size:13px}',
            '#cu-icra .i-gt-rapor li{margin:2px 0}',
            '#cu-icra .i-gt-uyarilar{max-height:160px;overflow:auto;font-size:12px;color:var(--i-soluk);margin:4px 0 0;padding-left:18px}',
            '#cu-icra .i-gt details summary{cursor:pointer;color:var(--i-soluk);font-size:12.5px;margin-top:6px}',
            // Borçlunun tebligat satırları
            '#cu-icra td.tb{font-size:13px}',
            '#cu-icra tr.ek td{border-top:1px dashed var(--i-kenar)}',
            '#cu-icra td.tb.sol{opacity:.45}',
            '#cu-icra td.tb .uzun{display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden;max-width:300px}',
            '#cu-icra .mz{display:inline-block;padding:1px 9px;border-radius:999px;font-size:12.5px;white-space:nowrap;background:color-mix(in srgb,#4f8a57 18%,var(--i-yuzey));color:color-mix(in srgb,#4f8a57 80%,var(--i-yazi))}',
            '#cu-icra .silik{color:var(--i-silik)}',
            '#cu-icra .i-dahasi{border:0;background:none;color:var(--i-vurgu-koyu);cursor:pointer;padding:0;font-size:12.5px}',
            '#cu-icra .i-dahasi:hover{text-decoration:underline}',
            '#cu-icra .dno{font-weight:600;white-space:nowrap}#cu-icra .alt{color:var(--i-soluk);font-size:12px}',
            '#cu-icra .chip{display:inline-block;padding:1px 9px;border-radius:999px;font-size:12.5px;font-weight:500;white-space:nowrap;',
            'background:color-mix(in srgb,var(--r) 18%,var(--i-yuzey));color:color-mix(in srgb,var(--r) 80%,var(--i-yazi))}',
            '#cu-icra .tarih{color:var(--i-silik);font-size:12px;margin-left:5px;white-space:nowrap}',
            '#cu-icra .uyari{color:#c06a2f;font-size:12px}',
            '#cu-icra .i-sayfa{display:flex;justify-content:space-between;align-items:center;color:var(--i-soluk);font-size:12.5px;margin-top:10px;gap:8px}',
            '#cu-icra .i-bilgi{background:var(--i-yuzey);border:1px dashed var(--i-kenar);border-radius:12px;padding:22px;color:var(--i-soluk);text-align:center}',
            '#cu-icra .i-hata{background:color-mix(in srgb,#b84444 12%,var(--i-yuzey));color:color-mix(in srgb,#b84444 80%,var(--i-yazi));border-radius:10px;padding:9px 13px;margin-bottom:12px}',
            '#cu-icra .i-panel{width:470px;max-width:100%;background:var(--i-yuzey);border-left:1px solid var(--i-kenar);display:flex;flex-direction:column;box-shadow:-10px 0 24px rgba(0,0,0,.06)}',
            '#cu-icra .i-panel .p-bas{padding:14px 18px 0}',
            '#cu-icra .i-panel h2{font-size:17px;margin:0}',
            '#cu-icra .i-rozet{display:flex;gap:6px;flex-wrap:wrap;margin:8px 0 10px}',
            '#cu-icra .i-gruplar{display:flex;gap:6px;flex-wrap:wrap;margin:6px 0 8px}',
            '#cu-icra .i-grup{border:1px solid var(--i-kenar);background:var(--i-yuzey);color:var(--i-yazi);border-radius:999px;padding:3px 11px;font-size:12.5px;cursor:pointer}',
            '#cu-icra .i-grup b{color:var(--i-soluk);font-weight:600}',
            '#cu-icra .i-grup.aktif{background:var(--cu-vurgu,#3f7fa3);border-color:var(--cu-vurgu,#3f7fa3);color:var(--cu-vurgu-uzeri,#fff)}',
            '#cu-icra .i-grup.aktif b{color:inherit}',
            '#cu-icra .h89{display:inline-flex;gap:3px}',
            '#cu-icra .i-rozet span{background:var(--i-zemin);color:var(--i-soluk);border-radius:999px;padding:2px 10px;font-size:12.5px}',
            '#cu-icra .i-panel .p-ic{flex:1;overflow:auto;padding:12px 18px 18px}',
            '#cu-icra .i-kart{border:1px solid var(--i-kenar);border-radius:12px;padding:11px 13px;margin-bottom:10px}',
            '#cu-icra .i-kart .b{display:flex;justify-content:space-between;gap:8px;align-items:flex-start}',
            '#cu-icra .i-kart .ad{font-weight:600}',
            '#cu-icra .sat{display:flex;justify-content:space-between;gap:12px;font-size:13px;margin-top:5px;color:var(--i-soluk)}',
            '#cu-icra .sat b{color:var(--i-yazi);font-weight:500;text-align:right}',
            '#cu-icra .i-ilerleme{display:flex;gap:3px;margin:9px 0 2px}',
            '#cu-icra .i-ilerleme i{flex:1;height:5px;border-radius:3px;background:var(--i-kenar)}',
            '#cu-icra .i-ilerleme i.d{background:var(--i-vurgu)}',
            '#cu-icra .i-eylem{display:flex;gap:6px;flex-wrap:wrap;margin-top:9px;align-items:center}',
            '#cu-icra .bolum{font-size:11.5px;letter-spacing:.04em;color:var(--i-silik);text-transform:uppercase;margin:6px 0 8px;font-weight:600}',
            '#cu-icra .i-olay{border-left:2px solid var(--i-kenar);padding:0 0 12px 12px;margin-left:4px;position:relative}',
            '#cu-icra .i-olay:before{content:"";position:absolute;left:-5px;top:5px;width:8px;height:8px;border-radius:50%;background:var(--i-vurgu)}',
            '#cu-icra .i-form{display:flex;gap:8px;flex-wrap:wrap;align-items:center;background:var(--i-serit);border-radius:10px;padding:10px;margin:10px 0}',
            '#cu-icra .i-iki{display:grid;grid-template-columns:minmax(0,1fr) 340px;gap:16px;align-items:start}',
            '#cu-icra .i-kutu{background:var(--i-yuzey);border:1px solid var(--i-kenar);border-radius:14px;padding:16px}',
            '#cu-icra .i-kutu h3{margin:0 0 4px;font-size:15px}#cu-icra .i-kutu p{margin:0 0 10px;color:var(--i-soluk);font-size:13px}',
            '#cu-icra .gun{display:inline-block;min-width:54px;text-align:center;border:1px solid var(--i-kenar);border-radius:8px;padding:2px 8px;background:var(--i-zemin);font-weight:600}',
            '#cu-icra .i-sarı{background:color-mix(in srgb,#b8901f 16%,var(--i-yuzey));border-radius:10px;padding:8px 12px;font-size:12.5px;margin-top:10px}',
            // Tebligatlar: dosya grupları kalın çizgiyle ayrılır; tekrarlanan dosya bilgileri soluk
            '#cu-icra table.teb th.suz{min-width:128px}#cu-icra table.teb th.suz-dosya_no{min-width:104px}',
            '#cu-icra table.teb td{font-size:13px}#cu-icra table.teb tbody.teb-dosya tr:first-child td{border-top:2px solid var(--i-kenar)}',
            '#cu-icra table.teb td.tekrar{color:var(--i-silik)}',
            '#cu-icra .i-sirala{cursor:pointer;user-select:none}#cu-icra .i-sirala:hover{color:var(--i-vurgu-koyu)}',
            '#cu-icra .i-ortu{position:fixed;inset:0;background:rgba(0,0,0,.35);z-index:20;display:flex;align-items:center;justify-content:center}',
            '#cu-icra .i-pencere{background:var(--i-yuzey);border-radius:14px;padding:14px 16px;width:min(1100px,calc(100vw - 24px));max-height:calc(100vh - 60px);display:flex;flex-direction:column;box-shadow:0 18px 50px rgba(0,0,0,.25)}',
            '#cu-icra .i-pencere-ic{flex:1;overflow:auto;min-height:120px}',
            '#cu-menu-icra .SidebarLabel{font-weight:600}',
            '#cu-menu-icra.on > li > a{background:var(--cu-vurgu-zemin,#e8ecf8)}',
            // Dosya Listesi: sade tablo, başlığa tıklayınca süzgeç penceresi (20.11.2026)
            '#cu-icra .i-seg{display:inline-flex;gap:2px;background:var(--i-serit);border:1px solid var(--i-kenar);border-radius:9px;padding:2px}',
            '#cu-icra .i-seg button{border:0;background:none;padding:5px 14px;border-radius:7px;color:var(--i-soluk);cursor:pointer}',
            '#cu-icra .i-seg button.on{background:var(--i-yuzey);color:var(--i-yazi);font-weight:600;box-shadow:0 1px 2px rgba(0,0,0,.08)}',
            '#cu-icra .i-dl-sayi{font-size:14px;color:var(--i-soluk)}#cu-icra .i-dl-sayi b{font-size:18px;font-weight:600;color:var(--i-yazi)}',
            '#cu-icra .i-dl-alan{display:flex;flex-direction:column;gap:5px}',
            '#cu-icra .i-dl-alan > span{font-size:12px;font-weight:500;color:var(--i-soluk)}',
            '#cu-icra .i-dl-tarih{display:flex;align-items:center;gap:6px;color:var(--i-silik)}',
            '#cu-icra .i-dl-suzgec{background:var(--i-yuzey);border:1px solid var(--i-kenar);border-radius:12px;padding:12px 16px;display:flex;flex-wrap:wrap;gap:14px 30px;align-items:flex-end;margin-bottom:12px}',
            '#cu-icra .i-dl-ipucu{display:flex;justify-content:space-between;gap:12px;flex-wrap:wrap;font-size:12.5px;color:var(--i-soluk);margin:0 2px 8px}',
            // Kutu ekran yüksekliğiyle sınırlı ve iki yönde kayar: yatay kaydırma çubuğu 100 satırlık tablonun dibinde
        // kalıp görünmüyordu (kullanıcı bulgusu 23.11.2026). Başlık satırı kayarken üstte durur.
        '#cu-icra .i-dl-kutu{overflow:auto;max-height:calc(100vh - 150px);min-height:220px;background:var(--i-yuzey);border:1px solid var(--i-kenar);border-radius:12px}',
        '#cu-icra .i-dl-kutu thead th{position:sticky;top:0;z-index:2;background:var(--i-yuzey)}',
        '#cu-icra .i-liste-cubuk{display:flex;flex-wrap:wrap;align-items:center;gap:8px 12px;padding:8px 22px;border-bottom:1px solid var(--i-kenar);background:var(--i-yuzey)}',
        '#cu-icra .i-liste-cubuk.secili{background:var(--i-serit);box-shadow:inset 4px 0 0 var(--cu-vurgu,#4f6bed)}',
        '#cu-icra .i-liste-cubuk select{max-width:min(360px,60vw)}',
        '#cu-icra .i-dl-kutu > table{border:0;border-radius:0;width:max-content;min-width:100%}',
        // Hücreler tek satır: tablo sütunları daraltıp satırları uzatmak yerine yana taşar, yatay çubukla kayar. Çubuk her
        // zaman görünür (kullanıcı isteği 23.11.2026: tablo daralıp sığdığı için çubuk hiç çıkmıyordu).
        '#cu-icra .i-dl-kutu{overflow-x:scroll;scrollbar-width:auto}',
        '#cu-icra .i-dl-kutu > table > thead > tr > th,#cu-icra .i-dl-kutu > table > tbody > tr > td{white-space:nowrap}',
        '#cu-icra .i-dl-kutu::-webkit-scrollbar{height:14px;width:12px}',
        '#cu-icra .i-dl-kutu::-webkit-scrollbar-thumb{background:var(--i-silik);border-radius:7px;border:3px solid var(--i-yuzey)}',
        '#cu-icra .i-dl-kutu::-webkit-scrollbar-track{background:var(--i-serit)}',
            '#cu-icra table.dl{border:0;border-radius:0;min-width:1100px}',
            '#cu-icra table.dl th{padding:0;vertical-align:middle}',
            '#cu-icra table.dl th > button{width:100%;display:flex;align-items:center;gap:6px;background:none;border:0;padding:9px 10px;font-weight:600;font-size:12px;color:var(--i-soluk);cursor:pointer;text-align:left;white-space:nowrap}',
            '#cu-icra table.dl th > button:hover{color:var(--i-yazi);background:color-mix(in srgb,var(--i-kenar) 35%,transparent)}',
            '#cu-icra table.dl th > button.dolu{color:var(--i-vurgu-koyu)}',
            '#cu-icra table.dl th > button .ok{color:var(--i-vurgu-koyu)}',
            '#cu-icra table.dl th > button svg{margin-left:auto;flex:none;opacity:.7}',
            '#cu-icra table.dl th > button .nokta{width:6px;height:6px;border-radius:50%;background:var(--i-vurgu);flex:none}',
            '#cu-icra table.dl td{vertical-align:middle;padding:10px}',
            '#cu-icra table.dl td.tek{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:220px}',
            '#cu-icra table.dl td.say{font-variant-numeric:tabular-nums;white-space:nowrap}',
            '#cu-icra table.dl td.rol .rol-taraf{font-size:12.5px;color:var(--i-soluk);margin-top:3px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:260px}',
            '#cu-icra table.dl .fazla{display:inline-block;margin-left:6px;font-size:12px;color:var(--i-soluk);background:var(--i-serit);border:1px solid var(--i-kenar);border-radius:6px;padding:0 6px}',
            '#cu-icra .i-dlp{position:fixed;z-index:6;width:300px;background:var(--i-yuzey);border:1px solid var(--i-kenar);border-radius:12px;box-shadow:0 10px 28px rgba(0,0,0,.18);padding:12px;display:flex;flex-direction:column;gap:10px;font-size:13px}',
            '#cu-icra .i-dlp input[type=text]{width:100%}',
            '#cu-icra .i-dlp input[type=checkbox]{accent-color:var(--i-vurgu)}',
            '#cu-icra .i-dlp .sirala{display:flex;gap:6px}#cu-icra .i-dlp .sirala .i-d{flex:1;padding:5px 8px;font-size:12.5px}',
            '#cu-icra .i-dlp .sirala .i-d.on{border-color:var(--i-vurgu);color:var(--i-vurgu-koyu);font-weight:600}',
            '#cu-icra .i-dlp .liste{max-height:260px;overflow:auto;border-top:1px solid var(--i-kenar);padding-top:6px;display:flex;flex-direction:column}',
            '#cu-icra .i-dlp label{display:flex;align-items:center;gap:8px;padding:5px 6px;border-radius:7px;cursor:pointer}',
            '#cu-icra .i-dlp label:hover{background:var(--i-vurgu-zemin)}',
            '#cu-icra .i-dlp label .ad{flex:1;min-width:0}#cu-icra .i-dlp label .adet{color:var(--i-silik);font-size:12px}',
            '#cu-icra .i-dlp label.grup{font-weight:500}#cu-icra .i-dlp label.pasif{opacity:.5;cursor:default}',
            '#cu-icra .i-dlp .ayrac{border-top:1px solid var(--i-kenar);margin:4px 0}',
            '#cu-icra .i-dlp .alt-cubuk{display:flex;justify-content:space-between;align-items:center;border-top:1px solid var(--i-kenar);padding-top:10px}',
            '#cu-icra .i-dlp .yok{padding:6px;color:var(--i-silik)}',
            // Yıl / ay ızgarası, süzgeç sırası penceresi, konan süzgeç etiketleri (21.11.2026)
            '#cu-icra .i-dlp.i-dlp-genis{width:360px}',
            '#cu-icra .i-dlp .bolum{display:flex;flex-direction:column;gap:6px}',
            '#cu-icra .i-dlp .bolum > b{font-size:12px;font-weight:600;color:var(--i-soluk)}',
            '#cu-icra .i-dlp .izgara{display:flex;flex-wrap:wrap;gap:4px}',
            '#cu-icra .i-dlp .izgara .i-d{display:inline-flex;align-items:baseline;gap:5px;padding:4px 9px}',
            '#cu-icra .i-dlp .izgara .i-d small{font-size:11px;color:var(--i-silik)}',
            '#cu-icra .i-dlp .izgara .i-d.on{border-color:var(--i-vurgu);background:var(--i-vurgu-zemin);color:var(--i-vurgu-koyu);font-weight:600}',
            '#cu-icra .i-dlp .izgara .i-d.on small{color:inherit}',
            '#cu-icra .i-dlp .izgara .i-d.bos:not(.on){opacity:.5}',
            '#cu-icra .i-dlp .sira{display:flex;flex-direction:column;gap:4px;border-top:1px solid var(--i-kenar);padding-top:8px}',
            '#cu-icra .i-dlp .sira .sat{display:flex;align-items:center;gap:6px;padding:6px 6px;border:1px solid var(--i-kenar);border-radius:8px;background:var(--i-yuzey)}',
            '#cu-icra .i-dlp .sira .sat[draggable=true]{cursor:grab}',
            '#cu-icra .i-dlp .sira .sat.sabit{background:var(--i-serit)}',
            '#cu-icra .i-dlp .sira .sat.tasiniyor{opacity:.45}',
            '#cu-icra .i-dlp .sira .sat.hedef{border-color:var(--i-vurgu);box-shadow:inset 0 2px 0 var(--i-vurgu)}',
            '#cu-icra .i-dlp .sira .tut{color:var(--i-silik);letter-spacing:-2px;width:14px;flex:none;text-align:center}',
            '#cu-icra .i-dlp .sira .no{color:var(--i-soluk);font-variant-numeric:tabular-nums;flex:none}',
            '#cu-icra .i-dlp .sira .ad{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}',
            '#cu-icra .i-dlp .sira .ad b{font-weight:600}',
            '#cu-icra .i-dlp .sira button{border:0;background:none;color:var(--i-soluk);cursor:pointer;padding:2px 5px;border-radius:5px;flex:none}',
            '#cu-icra .i-dlp .sira button:hover:not(:disabled){background:var(--i-vurgu-zemin);color:var(--i-vurgu-koyu)}',
            '#cu-icra .i-dlp .sira button:disabled{opacity:.3;cursor:default}',
            '#cu-icra .i-dl-tarih .i-dl-yilay{display:inline-flex;align-items:center;gap:6px;max-width:260px}',
            '#cu-icra .i-dl-tarih .i-dl-yilay span{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}',
            '#cu-icra .i-d.dolu{border-color:var(--i-vurgu);color:var(--i-vurgu-koyu)}',
            '#cu-icra .i-dl-suzgec-sag{margin-left:auto;display:flex;align-items:center;gap:14px}',
            '#cu-icra .i-dl-suzgec-sag .i-d{display:inline-flex;align-items:center;gap:6px}',
            '#cu-icra .i-dl-secili{margin:-4px 0 10px}',
            '#cu-icra .i-dl-secili .et{padding:0;gap:0;cursor:default}',
            '#cu-icra .i-dl-secili .et button{border:0;background:none;color:inherit;cursor:pointer;font:inherit}',
            '#cu-icra .i-dl-secili .et-ad{display:inline-flex;gap:6px;align-items:center;padding:3px 4px 3px 11px;min-width:0}',
            '#cu-icra .i-dl-secili .et-ad b{font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:280px}',
            '#cu-icra .i-dl-secili .et-x{opacity:.7;padding:3px 9px 3px 5px}#cu-icra .i-dl-secili .et-x:hover{opacity:1}',
            '#cu-icra .i-gt-secenek{display:flex;align-items:center;justify-content:space-between;gap:14px;padding:8px 10px;border:1px solid var(--i-kenar);border-radius:9px;margin-bottom:6px;background:var(--i-yuzey)}',
            '#cu-icra .i-gt-secenek .i-d{flex:none;white-space:nowrap}',
            // Dar ekran: liste satırları kart gibi, panel tam sayfa.
            'html.cu-dar #cu-icra .i-bas,html.cu-dar #cu-icra .i-sekmeler{padding-left:12px;padding-right:12px}',
            'html.cu-dar #cu-icra .i-ana{padding:12px}',
            'html.cu-dar #cu-icra .i-bas{gap:4px 10px;padding-top:10px}',
            'html.cu-dar #cu-icra .i-bas h1{font-size:17px}',
            'html.cu-dar #cu-icra .i-durum{order:3;flex-basis:100%}',
            'html.cu-dar #cu-icra input[type=search]{flex:1 1 100%;max-width:none}',
            'html.cu-dar #cu-icra .i-panel{position:absolute;inset:0;width:auto;border-left:0;z-index:2}',
            'html.cu-dar #cu-icra .i-iki{grid-template-columns:1fr}',
            'html.cu-dar #cu-icra table.liste thead{display:none}',
            'html.cu-dar #cu-icra table.liste tbody{display:block;border-top:1px solid var(--i-kenar);padding:6px 0}',
            'html.cu-dar #cu-icra table.liste tbody:first-of-type{border-top:0}',
            // Dar ekranda hücreler alt alta: Kesinleşme kendi tebligatının altında kalır (geniş tabloda borçludan sonra).
            'html.cu-dar #cu-icra table.liste tr{display:flex;flex-direction:column}',
            'html.cu-dar #cu-icra table.liste td.kes{order:9}',
            'html.cu-dar #cu-icra table.liste td{display:block;border:0!important;padding:2px 12px}',
            'html.cu-dar #cu-icra table.liste td.tb{margin-left:12px;padding-left:10px;border-left:2px solid var(--i-kenar)!important}',
            'html.cu-dar #cu-icra table.liste td.tb.tb0{margin-top:8px}',
            // ── Telefon (2026-11-23, kullanıcı bulgusu: dosya kartı üstteki bantların altında ~260 px'e, yatıkta 80 px'e
            // sıkışıyordu; işlem yapılamıyordu). Dosya kartı tam ekran ve bütünüyle kayar; başlık ince, sekmeler tek satır,
            // dosya listesi bandı tek satır ("⋯" ile açılır); araçlar ve süzgeçler "Araçlar" düğmesinin arkasında; liste
            // kayınca başlık ve liste bandı gizlenir; Dosya Listesi / Kesinleştirme tabloları kart olur.
            '#cu-icra .m-goster,#cu-icra .m-ac,#cu-icra .i-liste-dahasi{display:none}',
            'html.cu-dar #cu-icra .i-panel{position:fixed;top:var(--cu-nav-yuk,60px);bottom:var(--cu-cubuk-yuk,44px);left:0;right:0;z-index:5;display:block;overflow-y:auto;overscroll-behavior:contain}',
            'html.cu-dar #cu-icra .i-panel .p-ic{overflow:visible}',
            'html.cu-dar #cu-icra .i-panel .p-sekmeler{position:sticky;top:0;z-index:1;background:var(--i-yuzey)}',
            '#cu-icra .i-panel .p-sekmeler{padding:0 0 0 18px;margin:0}',
            'html.cu-mobil #cu-icra .i-panel .p-bas{padding:10px 12px 0}',
            'html.cu-mobil #cu-icra .i-panel .p-sekmeler{padding-left:4px}',
            'html.cu-mobil #cu-icra .i-panel .p-ic{padding:10px 12px 14px}',
            'html.cu-mobil #cu-icra .i-panel .i-rozet{margin:6px 0 8px}',
            'html.cu-mobil #cu-icra .m-gizle{display:none}',
            'html.cu-mobil #cu-icra .m-goster{display:inline}',
            'html.cu-mobil #cu-icra .i-bas{padding:6px 12px 4px;gap:8px;flex-wrap:nowrap}',
            'html.cu-mobil #cu-icra .i-bas h1{font-size:16px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}',
            'html.cu-mobil #cu-icra .i-bas .i-durum{display:none}',
            'html.cu-mobil #cu-icra .i-bas .i-d{padding:4px 10px;min-width:34px}',
            'html.cu-dar #cu-icra .i-sekmeler{flex-wrap:nowrap;overflow-x:auto;scrollbar-width:none}',
            'html.cu-dar #cu-icra .i-sekmeler::-webkit-scrollbar{display:none}',
            'html.cu-dar #cu-icra .i-sekmeler button{white-space:nowrap;flex:none}',
            'html.cu-mobil #cu-icra .i-sekmeler{padding-top:0}',
            'html.cu-mobil #cu-icra .i-sekmeler button{padding:8px 10px}',
            'html.cu-mobil #cu-icra .i-liste-cubuk{padding:6px 12px;flex-wrap:nowrap;gap:8px}',
            'html.cu-mobil #cu-icra .i-liste-cubuk select{flex:1;max-width:none}',
            'html.cu-mobil #cu-icra .i-liste-dahasi{display:inline-block;flex:none}',
            'html.cu-mobil #cu-icra .i-liste-cubuk:not(.acik) > :not(select):not(.i-liste-dahasi){display:none}',
            'html.cu-mobil #cu-icra .i-liste-cubuk.acik{flex-wrap:wrap}',
            'html.cu-mobil #cu-icra.kaydi > .i-bas,html.cu-mobil #cu-icra.kaydi > #cu-icra-liste{display:none}',
            'html.cu-mobil #cu-icra .m-ac{display:inline-block}',
            'html.cu-mobil #cu-icra .i-ana:not(.m-acik) .m-ek{display:none!important}',
            'html.cu-mobil #cu-icra .i-arac{gap:8px;margin-bottom:10px}',
            'html.cu-mobil #cu-icra .i-arac input[type=search]{flex:1 1 150px}',
            'html.cu-mobil #cu-icra .i-arac .i-dl-sayi{flex:1}',
            'html.cu-mobil #cu-icra .i-asamalar{margin-bottom:10px}',
            'html.cu-mobil #cu-icra .i-dl-kutu{overflow:visible;max-height:none;min-height:0}',
            'html.cu-mobil #cu-icra table.dl{display:block;min-width:0;width:100%}',
            'html.cu-mobil #cu-icra table.dl thead{display:block;overflow-x:auto;scrollbar-width:none;border-bottom:1px solid var(--i-kenar)}',
            'html.cu-mobil #cu-icra table.dl thead tr{display:flex;flex-wrap:nowrap;gap:6px;padding:8px 10px}',
            'html.cu-mobil #cu-icra table.dl th{flex:none;border:0;background:none}',
            'html.cu-mobil #cu-icra table.dl th > button{width:auto;border:1px solid var(--i-kenar);border-radius:999px;padding:5px 10px;gap:5px}',
            'html.cu-mobil #cu-icra table.dl th > button svg{margin-left:0}',
            'html.cu-mobil #cu-icra table.dl tbody{display:block;border-top:1px solid var(--i-kenar)}',
            'html.cu-mobil #cu-icra table.dl tbody:first-of-type{border-top:0}',
            'html.cu-mobil #cu-icra table.dl tr{display:flex;flex-wrap:wrap;gap:2px 10px;padding:10px 12px}',
            'html.cu-mobil #cu-icra table.dl td{display:block;flex:1 1 100%;min-width:0;padding:0;border:0!important;max-width:none;font-size:13px}',
            'html.cu-mobil #cu-icra table.dl td[data-s]::before{content:attr(data-ad) ": ";color:var(--i-silik);font-size:12px}',
            'html.cu-mobil #cu-icra table.dl td[data-s=dosya_no]{order:-2;flex:1 1 auto;font-weight:600;font-size:14.5px}',
            'html.cu-mobil #cu-icra table.dl td[data-s=acilis]{order:-2;flex:0 0 auto;margin-left:auto;color:var(--i-soluk)}',
            'html.cu-mobil #cu-icra table.dl td[data-s=birim]{order:-1;color:var(--i-soluk)}',
            'html.cu-mobil #cu-icra table.dl td[data-s=dosya_no]::before,html.cu-mobil #cu-icra table.dl td[data-s=birim]::before,html.cu-mobil #cu-icra table.dl td[data-s=acilis]::before,html.cu-mobil #cu-icra table.dl td[data-s=grup]::before,html.cu-mobil #cu-icra table.dl td[data-s=asama]::before{content:none}',
            'html.cu-mobil #cu-icra table.dl td.bos{display:none}',
            'html.cu-mobil #cu-icra .i-ana > .i-gruplar{flex-wrap:nowrap;overflow-x:auto;scrollbar-width:none;margin:0 -12px 8px;padding:0 12px}',
            'html.cu-mobil #cu-icra .i-ana > .i-gruplar .i-grup{flex:none;white-space:nowrap}',
            'html.cu-mobil #cu-icra .i-secim > span.alt:not(.i-secim-sayi){display:none}',
            'html.cu-dar #cu-icra table.liste{display:block;width:100%;min-width:0}',
            'html.cu-dar #cu-icra table.liste td{white-space:normal;overflow-wrap:anywhere;min-width:0}',
        ].join('\n');
        document.head.appendChild(e('style', { id: 'cu-icra-stil', metin: css }));
    }

    // ── Durum ──
    const d = {
        sekme: 'dosyalar', filtre: { q: '', asama: '', durum: 'acik', sayfa: 1, kesinlesme: '', tur: '', teblig: '', mazbata: '',
                                  birim: '', dosya_no: '', alacakli: '', borclu: '', yil: '', dosya_tur: '', acilis_bas: '', acilis_son: '' },
        ozet: null, liste: null, secili: null, panelSekme: 'borclular', hata: '',
        kutu: null, kutuMetin: '', kutuYazildi: false,   // açık başlık kutusu (anahtar), içindeki yazı
        tumTebligat: {},                                 // bag → true: borçlunun bütün tebligatları açık
        secListe: (function() { try { const v = parseInt(localStorage.getItem('cu-icra-dosya-listesi'), 10); return v > 0 ? v : null; } catch (x) { return null; } })(),
        listeler: null,                                  // dosya listeleri (sunucudan; seçili liste bütün dosya sekmelerinde ortak)
        gt: null, gtAcik: false,                         // UYAP'tan güncelleme: son tarama, panel açık mı
        gtForm: { ptt: true },                       // toplu tarama seçenekleri (yeniden çizimde korunur)
        // Seçili dosyalar (veritabanı kimliği). Süzgeç değişince korunur; ana ekran düğmeleri seçim varsa yalnız bunlara işler.
        secim: new Set()
    };
    let kok = null, govde = null, araZaman = null, kutuZaman = null;
    // Başlık süzgeçleri. kodlu: değer sabit bir koddur (yalnız listeden seçilir); değilse yazıyla "içinde geçen" de süzülür.
    // baslik: sütun başlığının adı (kutunun adından farklıysa). Sıra = tablo sütunları (Takip aşaması 5. kutudan sonra).
    // Sütun sırası (kullanıcı isteği 19.11.2026): daire, dosya no, alacaklı, borçlu, kesinleşme, takip aşaması, tebligat …
    // Listeden seçimde "Tümünü seç" sonra bazılarını kaldırmak da olur: süzgeç "!" + hariç tutulanlar (19.11.2026).
    const SUZGEC = [
        { k: 'birim', ad: 'İcra dairesi', kodlu: false },
        { k: 'dosya_no', ad: 'Dosya no', kodlu: false },
        { k: 'alacakli', ad: 'Alacaklı', kodlu: false },
        { k: 'borclu', ad: 'Borçlu', kodlu: false },
        // Kesinleşme tebligat başınadır (ödeme emri: takip türünün süresi; 103 / kıymet takdiri: 7 gün; diğerlerinde yok).
        // Süzgeç borçlunun ödeme emri kesinleşmesine göre süzer.
        { k: 'kesinlesme', ad: 'Kesinleşme', kodlu: true },
        { k: 'tur', ad: 'Tebligat türü', kodlu: false },
        { k: 'teblig', ad: 'Tebliğ durumu', kodlu: false },
        { k: 'mazbata', ad: 'Mazbata', kodlu: true }
    ];
    // Tablo üstündeki künye kutuları (sütunu olmayanlar).
    const UST_SUZGEC = [
        { k: 'yil', ad: 'Dosya yılı', kodlu: true },
        { k: 'dosya_tur', ad: 'Dosya türü', kodlu: true }
    ];
    const TUM_SUZGEC = SUZGEC.concat(UST_SUZGEC);
    const DURUMLAR = [['acik', 'Açık dosyalar'], ['kapali', 'Kapalı dosyalar'], ['', 'Bütün dosyalar']];
    function listeSorgusu() {
        const f = d.filtre;
        const s = { q: f.q, asama: f.asama, durum: f.durum, acilis_bas: f.acilis_bas, acilis_son: f.acilis_son,
                    sayfa: String(f.sayfa), boyut: '50' };
        TUM_SUZGEC.forEach(function(z) { s[z.k] = f[z.k]; });
        if (d.secListe) s.liste = String(d.secListe);
        return new URLSearchParams(s).toString();
    }
    const TEBLIGAT_GORUNEN = 3;                  // satırda ilk gösterilen tebligat sayısı (kalanı "+N tebligat daha")
    function trKucuk(x) {
        return String(x || '').replace(/İ/g, 'i').replace(/I/g, 'ı').toLowerCase()
            .replace(/ı/g, 'i').replace(/ş/g, 's').replace(/ğ/g, 'g').replace(/ü/g, 'u').replace(/ö/g, 'o').replace(/ç/g, 'c');
    }
    let gorunum = 'kapali';                     // 'kapali' | 'acik' | 'kucuk'
    const icradanAcilan = new Set();             // bu sayfadan "UYAP'ta aç" ile açılan dosya kartları (anahtar)
    function kartAcikMi() {
        return !!Array.from(document.querySelectorAll('.dosya-sorgula-popup .dx-popup-title'))
            .find(function(x) { return x.getClientRects().length > 0; });
    }
    function durumSakla() {
        try {
            if (gorunum === 'kapali') sessionStorage.removeItem(ACIK_ANAHTAR);
            else sessionStorage.setItem(ACIK_ANAHTAR, gorunum === 'acik' ? '1' : 'k');
        } catch (x) {}
    }

    // ── Menü girişi ──
    function menuGirisi() {
        const yan = document.getElementById('av-sidebar-wrapper');
        if (!yan || document.getElementById('cu-menu-icra')) return;
        const ust = yan.querySelector('.sidebar-top-part') || yan.firstElementChild || yan;
        const ul = e('ul', { id: 'cu-menu-icra', sinif: 'menu-list' + (gorunum === 'acik' ? ' on' : '') }, [
            e('li', { sinif: 'SidebarLink' }, [
                e('a', { href: '#', title: 'Büro veritabanındaki icra dosyalarının takip aşamaları, kesinleşme ve tebliğ durumları',
                         onclick: function(ev) { ev.preventDefault(); ac(true); } },
                  [e('span', { sinif: 'SidebarLabel', metin: 'İcra Takip Süreçleri' })])
            ])
        ]);
        ust.insertBefore(ul, ust.firstChild);
    }

    // ── Sayfa ──
    function yerlestir() {
        if (!kok) return;
        const ic = document.getElementById('page-content-wrapper');
        let sol = 0;
        if (ic && !dar()) sol = Math.max(0, Math.round(ic.getBoundingClientRect().left));
        if (kok.style.left !== sol + 'px') kok.style.left = sol + 'px';
    }

    // one: kullanıcı açtı (menü, görev çubuğu) → ekrandaki dosya kartı görev çubuğuna küçülür, sayfa görünsün.
    function ac(one) {
        stilKur();
        gorunum = 'acik';
        durumSakla();
        if (!kok) {
            kok = e('div', { id: 'cu-icra', role: 'region', 'aria-label': 'İcra Takip Süreçleri' });
            document.body.appendChild(kok);
            kok.addEventListener('scroll', mobilKaydirma, true);
            ciz();
            yukle();
            gtYokla(true);
        }
        digerSayfalariIndir();
        kok.style.display = '';
        if (one && kartAcikMi() && window.__cuGorunurKartiKucult) window.__cuGorunurKartiKucult();
        const m = document.getElementById('cu-menu-icra');
        if (m) m.classList.add('on');
        yerlestir();
        gorevGuncelle(true);
    }

    // Görev çubuğuna küçült: sayfa silinmez, yalnız gizlenir (durum yerinde kalır).
    // İş Listesi ile İcra Takip Süreçleri aynı alanı kaplayan iki ayrı pencere: aynı anda yalnız biri önde
    // durur. Biri açılınca öbürü görev çubuğuna iner (kullanıcı bildirimi 2026-11-23: İcra açıkken İş
    // Listesi'ne basınca İcra önde kalıyordu).
    const sayfalar = window.__cuSayfalar = window.__cuSayfalar || {};
    sayfalar['icra'] = function() { if (gorunum === 'acik') kucult(); };
    function digerSayfalariIndir() {
        Object.keys(sayfalar).forEach(function(k) { if (k !== 'icra') try { sayfalar[k](); } catch (x) {} });
    }
    function kucult() {
        if (gorunum === 'kapali') return;
        gorunum = 'kucuk';
        durumSakla();
        icradanAcilan.clear();                   // kullanıcı kendisi indirdi: kart kapanınca geri getirme
        if (kok) kok.style.display = 'none';
        const m = document.getElementById('cu-menu-icra');
        if (m) m.classList.remove('on');
        gorevGuncelle(true);
    }

    function kapat() {
        gorunum = 'kapali';
        durumSakla();
        icradanAcilan.clear();
        if (kok) kok.remove();
        kok = null; govde = null;
        d.kutu = null; d.kutuMetin = ''; d.kutuYazildi = false;
        const m = document.getElementById('cu-menu-icra');
        if (m) m.classList.remove('on');
        gorevGuncelle(true);
    }

    // ── Görev çubuğu düğmesi ──
    // "on" (vurgulu): sayfa açık ve üstünde dosya kartı yok. Tıklama: öndeyse küçült, değilse öne getir.
    let gorevImza = null;
    function onde() { return gorunum === 'acik' && !kartAcikMi(); }
    function gorevTikla() { if (onde()) kucult(); else ac(true); }
    function gorevGuncelle(zorla) {
        if (!window.CU || !window.CU.gorev) return;
        const on = onde();
        const imza = gorunum + '|' + on;
        if (!zorla && imza === gorevImza) return;
        gorevImza = imza;
        if (gorunum === 'kapali') { window.CU.gorev.ayarla([], 'icra'); return; }
        window.CU.gorev.ayarla([{
            kimlik: 'icra', ad: 'İcra Takip Süreçleri', ikon: '📋', on: on, kucuk: !on,
            tikla: gorevTikla, kapat: kapat,
            menu: function() { return [[[onde() ? 'Küçült' : 'Öne getir', gorevTikla]]]; },
        }], 'icra');
    }

    function ciz() {
        if (!kok) return;
        bos(kok);
        const durum = e('div', { sinif: 'i-durum', id: 'cu-icra-durum' });
        kok.appendChild(e('div', { sinif: 'i-bas' }, [
            e('h1', { metin: 'İcra Takip Süreçleri' }), durum, e('div', { sinif: 'i-bosluk' }),
            e('button', { sinif: 'i-d', metin: '–', title: 'Görev çubuğuna küçült (kaldığınız yer korunur)', 'aria-label': 'Küçült',
                          stil: 'min-width:38px;font-weight:700', onclick: kucult }),
            e('button', { sinif: 'i-d', title: 'Sayfayı kapat (görev çubuğundan da çıkar)', 'aria-label': 'Kapat', onclick: kapat },
              [e('span', { sinif: 'm-gizle', metin: 'Kapat' }), e('span', { sinif: 'm-goster', metin: '✕' })])
        ]));
        const sekmeler = [['dosyalar', 'Dosya Listesi'], ['kesin', 'Kesinleştirme'], ['surec', 'Süreçler'], ['tebligat', 'Tebligatlar'],
                          ['sure', 'Kesinleşme Süreleri'], ['tatil', 'Resmî Tatiller']];
        kok.appendChild(e('div', { sinif: 'i-sekmeler', role: 'tablist' }, sekmeler.map(function(s) {
            return e('button', { sinif: d.sekme === s[0] ? 'on' : '', role: 'tab', metin: s[1],
                                 onclick: function() { dlKapat(); d.sekme = s[0]; kutuBirak(); ciz(); yukle(); } });
        })));
        if (dosyaSekmesi()) {
            kok.appendChild(e('div', { id: 'cu-icra-liste' }, [listeCubugu()]));
            if (d.listeler === null) listeleriYukle().then(listeCubuguYenile);
        }
        govde = e('div', { sinif: 'i-govde' });
        kok.appendChild(govde);
        kok.classList.remove('kaydi');
        const onSekme = kok.querySelector('.i-sekmeler button.on');
        if (onSekme && dar()) { const sk = onSekme.parentNode; sk.scrollLeft = Math.max(0, onSekme.offsetLeft - 40); }
        durumYaz();
    }
    // Telefonda liste kayınca başlık ve dosya listesi bandı gizlenir, en üste dönünce geri gelir. Kısa listede gizleme
    // yapılmaz (gizleyince kaydırma payı biter, bant geri gelir: titreme).
    function mobilKaydirma(ev) {
        const a = ev.target;
        if (!kok || !a || !a.classList || !a.classList.contains('i-ana') || !document.documentElement.classList.contains('cu-mobil')) return;
        if (a.scrollTop > 80 && a.scrollHeight - a.clientHeight > 260) kok.classList.add('kaydi');
        else if (a.scrollTop < 8) kok.classList.remove('kaydi');
    }
    // Telefonda araçlar ve süzgeçler (m-ek) "Araçlar" düğmesinin arkasında; durum sekmeler arasında korunur.
    function mEk(x) { if (x && x.classList) x.classList.add('m-ek'); return x; }
    function mAracDugmesi(ana) {
        if (d.mArac) ana.classList.add('m-acik');
        const b = e('button', { sinif: 'i-d kucuk m-ac', type: 'button', 'aria-expanded': d.mArac ? 'true' : 'false', metin: d.mArac ? 'Araçları gizle' : 'Araçlar ▾',
            title: 'Güncelleme düğmeleri ve süzgeçler' });
        b.addEventListener('click', function() {
            d.mArac = !d.mArac;
            ana.classList.toggle('m-acik', d.mArac);
            b.textContent = d.mArac ? 'Araçları gizle' : 'Araçlar ▾';
            b.setAttribute('aria-expanded', d.mArac ? 'true' : 'false');
        });
        return b;
    }
    // Kart görünümü için hücrelere sütun kimliği ve adı; boş ("—") hücreler telefonda gizlenir.
    function hucreEtiketle(tr, sutunlar) {
        Array.prototype.forEach.call(tr.children, function(td, i) {
            const c = sutunlar[i];
            if (!c) return;
            td.setAttribute('data-s', c.k || c.s);
            td.setAttribute('data-ad', c.ad);
            if (td.textContent.trim() === '—') td.classList.add('bos');
        });
        return tr;
    }

    // ── Dosya listeleri (23.11.2026, kullanıcı isteği; sunucu: src/icra_listeler.rs) ──
    // Adlı, SABİT dosya grupları (büro ortak). Seçili liste bütün dosya sekmelerinde ortaktır: her sekme yalnız o listedeki
    // dosyaları gösterir, sekmenin kendi süzgeçleri listenin içinde çalışır; toplu tarama da (kapsamı süzgeçten gelir)
    // liste dışına istek atmaz. Dosya Listesi'nde süzgeçtekiler, Süreçler / Tebligatlar'da seçililer listeye eklenir.
    const LISTE_ANAHTARI = 'cu-icra-dosya-listesi';
    function dosyaSekmesi() { return ['dosyalar', 'kesin', 'surec', 'tebligat'].indexOf(d.sekme) >= 0; }
    async function listeleriYukle() {
        try { d.listeler = await istek('listeler'); } catch (x) { d.listeler = []; bildir(x.message); }
        // Silinmiş liste seçili kalmasın.
        if (d.secListe && !d.listeler.some(function(l) { return l.id === d.secListe; })) listeSecimiYaz(null);
    }
    function listeSecimiYaz(id) {
        d.secListe = id || null;
        try { if (id) localStorage.setItem(LISTE_ANAHTARI, String(id)); else localStorage.removeItem(LISTE_ANAHTARI); } catch (x) {}
    }
    function seciliListe() { return (d.listeler || []).find(function(l) { return l.id === d.secListe; }) || null; }
    function listeSec(id) {
        listeSecimiYaz(id);
        // Liste dışındaki dosyalar seçili kalırsa toplu tarama onlara giderdi: seçim temizlenir.
        d.secim.clear();
        [d.filtre, d.dl && d.dl.filtre, d.ks && d.ks.filtre, d.teb && d.teb.filtre].forEach(function(f) { if (f) f.sayfa = 1; });
        ciz();
        yukle();
    }
    function listeCubuguYenile() {
        const t = kok && kok.querySelector('#cu-icra-liste');
        if (t) t.replaceChildren(listeCubugu());
    }
    function listeCubugu() {
        const ls = d.listeler || [];
        const secili = seciliListe();
        const kutu = e('div', { sinif: 'i-liste-cubuk' + (secili ? ' secili' : ''), role: 'group', 'aria-label': 'Dosya listesi' });
        const sec = e('select', { 'aria-label': 'Görüntülenecek dosya listesi' }, [e('option', { value: '', metin: 'Bütün dosyalar' })].concat(ls.map(function(l) {
            return e('option', { value: String(l.id), selected: !!secili && l.id === secili.id, metin: l.ad + ' (' + l.dosya_sayisi + ' dosya)' });
        })));
        sec.addEventListener('change', function() { listeSec(parseInt(sec.value, 10) || null); });
        if (d.listeAcik) kutu.classList.add('acik');
        kutu.appendChild(e('b', { metin: 'Dosya listesi' }));
        kutu.appendChild(sec);
        const dahasi = e('button', { sinif: 'i-d kucuk i-liste-dahasi', type: 'button', metin: d.listeAcik ? '▴' : '⋯',
            title: 'Liste işlemleri', 'aria-label': 'Liste işlemleri', 'aria-expanded': d.listeAcik ? 'true' : 'false' });
        dahasi.addEventListener('click', function() {
            d.listeAcik = !d.listeAcik;
            kutu.classList.toggle('acik', d.listeAcik);
            dahasi.textContent = d.listeAcik ? '▴' : '⋯';
            dahasi.setAttribute('aria-expanded', d.listeAcik ? 'true' : 'false');
        });
        kutu.appendChild(dahasi);
        if (secili) {
            kutu.appendChild(e('button', { sinif: 'i-d kucuk', type: 'button', metin: '× Listeyi kaldır', title: 'Bütün dosyaları göster', onclick: function() { listeSec(null); } }));
            kutu.appendChild(e('span', { sinif: 'alt', metin: 'Yalnız bu listedeki ' + secili.dosya_sayisi + ' dosya (' + secili.acik_sayisi + ' açık); süzgeçler listenin içinde çalışır.'
                + (secili.aciklama ? ' · ' + secili.aciklama : '') }));
        } else if (d.listeler && !ls.length && d.sekme === 'dosyalar') {
            kutu.appendChild(e('span', { sinif: 'alt', metin: 'Henüz liste yok: süzgeçleyip "Süzgeçtekileri listeye ekle" ile oluşturun.' }));
        }
        kutu.appendChild(e('div', { sinif: 'i-bosluk' }));
        const dugme = function(metin, title, fn) { return e('button', { sinif: 'i-d kucuk', type: 'button', metin: metin, title: title, onclick: fn }); };
        if (d.sekme === 'dosyalar') {
            kutu.appendChild(dugme('Süzgeçtekileri listeye ekle…', 'Süzgeçteki bütün dosyaları (bütün sayfalar) yeni ya da mevcut bir listeye ekler', function() { listeyeEklePenceresi('suzgec'); }));
            if (secili) kutu.appendChild(dugme('Süzgeçtekileri listeden çıkar…', 'Süzgeçteki dosyaları bu listeden çıkarır', function() { listedenCikar('suzgec'); }));
        } else if (d.sekme === 'surec' || d.sekme === 'tebligat') {
            kutu.appendChild(dugme('Seçilileri listeye ekle…', 'Satırlarda işaretli dosyaları bir listeye ekler', function() { listeyeEklePenceresi('secim'); }));
            if (secili) kutu.appendChild(dugme('Seçilileri listeden çıkar…', 'İşaretli dosyaları bu listeden çıkarır', function() { listedenCikar('secim'); }));
        }
        kutu.appendChild(dugme('Listeleri yönet', 'Ad / açıklama değiştir, sil', listeYonet));
        return kutu;
    }
    // İşlenecek dosyalar: Dosya Listesi süzgecindekiler (bütün sayfalar) ya da satırlarda seçililer.
    async function listeKaynagi(kaynak) {
        if (kaynak === 'secim') {
            if (!d.secim.size) throw new Error('Önce satırlardaki kutularla dosya seçin.');
            return { ad: 'seçili ' + d.secim.size + ' dosya', idler: Array.from(d.secim) };
        }
        const idler = await istek('dosyalar_kimlikleri?' + dlSorgusu(true) + '&mod=');
        if (!idler.length) throw new Error('Süzgeçte dosya yok.');
        return { ad: 'süzgeçteki ' + idler.length + ' dosya', idler: idler };
    }
    function kucukPencere(baslik, cocuklar, genislik) {
        const ortu = e('div', { sinif: 'i-ortu', role: 'dialog', 'aria-modal': 'true', 'aria-label': baslik });
        const kapat = function() { ortu.remove(); };
        ortu.addEventListener('mousedown', function(ev) { if (ev.target === ortu) kapat(); });
        ortu.addEventListener('keydown', function(ev) { if (ev.key === 'Escape') kapat(); });
        ortu.appendChild(e('div', { sinif: 'i-pencere', stil: 'width:min(' + (genislik || 520) + 'px,calc(100vw - 24px))' },
            [e('div', { stil: 'font-weight:700;font-size:15px;margin-bottom:8px', metin: baslik })].concat(cocuklar)));
        kok.appendChild(ortu);
        return kapat;
    }
    async function listeDegisti(mesaj) {
        bildir(mesaj);
        await listeleriYukle();
        listeCubuguYenile();
        if (d.secListe) yukle();
    }
    async function listeyeEklePenceresi(kaynak) {
        let k;
        try { k = await listeKaynagi(kaynak); } catch (x) { bildir(x.message); return; }
        if (d.listeler === null) await listeleriYukle();
        const ls = d.listeler || [];
        const mevcut = e('select', { 'aria-label': 'Eklenecek liste' }, ls.map(function(l) {
            return e('option', { value: String(l.id), selected: l.id === d.secListe, metin: l.ad + ' (' + l.dosya_sayisi + ' dosya)' });
        }));
        const yeniAd = e('input', { type: 'text', placeholder: 'Yeni listenin adı (ör. Bankalar — Kesinleşme bekleyen)', 'aria-label': 'Yeni liste adı', stil: 'width:100%' });
        const aciklama = e('input', { type: 'text', placeholder: 'Açıklama (isteğe bağlı)', 'aria-label': 'Açıklama', stil: 'width:100%;margin-top:6px' });
        const secenek = function(deger, ad, ic) {
            const r = e('input', { type: 'radio', name: 'i-liste-hedef', value: deger, checked: deger === (ls.length ? 'mevcut' : 'yeni') });
            return e('div', { stil: 'margin:8px 0' }, [e('label', { stil: 'display:flex;gap:6px;align-items:center;font-weight:600' }, [r, ad]), e('div', { stil: 'margin:4px 0 0 22px' }, ic)]);
        };
        const ekle = e('button', { sinif: 'i-d ana', type: 'button', metin: 'Ekle' });
        const kapat = kucukPencere('Dosyaları listeye ekle', [
            e('div', { sinif: 'alt', metin: 'Eklenecek: ' + k.ad + '. Liste sabittir; sonradan süzgece uyan dosyalar kendiliğinden girmez.' }),
            ls.length ? secenek('mevcut', 'Mevcut listeye', [mevcut]) : null,
            secenek('yeni', 'Yeni liste', [yeniAd, aciklama]),
            e('div', { stil: 'display:flex;gap:8px;justify-content:flex-end;margin-top:10px' }, [
                e('button', { sinif: 'i-d', type: 'button', metin: 'Vazgeç', onclick: function() { kapat(); } }), ekle])
        ]);
        yeniAd.addEventListener('focus', function() { const r = kok.querySelector('input[name="i-liste-hedef"][value="yeni"]'); if (r) r.checked = true; });
        ekle.addEventListener('click', async function() {
            const hedef = (kok.querySelector('input[name="i-liste-hedef"]:checked') || {}).value;
            ekle.disabled = true;
            try {
                let id = parseInt(mevcut.value, 10);
                if (hedef === 'yeni') {
                    if (!yeniAd.value.trim()) { bildir('Yeni listenin adını yazın.'); ekle.disabled = false; yeniAd.focus(); return; }
                    id = (await istek('liste', { ad: yeniAd.value, aciklama: aciklama.value })).id;
                }
                const r = await istek('liste_ekle', { id: id, dosyalar: k.idler });
                kapat();
                await listeDegisti(r.eklenen + ' dosya listeye eklendi' + (k.idler.length > r.eklenen ? ' (' + (k.idler.length - r.eklenen) + ' dosya zaten listedeydi)' : '') + '; listede ' + r.toplam + ' dosya var.');
            } catch (x) { bildir(x.message); ekle.disabled = false; }
        });
    }
    async function listedenCikar(kaynak) {
        const l = seciliListe();
        if (!l) return;
        let k;
        try { k = await listeKaynagi(kaynak); } catch (x) { bildir(x.message); return; }
        const tamam = e('button', { sinif: 'i-d tehlike', type: 'button', metin: 'Listeden çıkar' });
        const kapat = kucukPencere('Listeden çıkar', [
            e('div', { metin: '"' + l.ad + '" listesinden ' + k.ad + ' çıkarılacak. Dosyaların kendisi silinmez.' }),
            e('div', { stil: 'display:flex;gap:8px;justify-content:flex-end;margin-top:12px' }, [
                e('button', { sinif: 'i-d', type: 'button', metin: 'Vazgeç', onclick: function() { kapat(); } }), tamam])
        ], 460);
        tamam.addEventListener('click', async function() {
            tamam.disabled = true;
            try {
                const r = await istek('liste_cikar', { id: l.id, dosyalar: k.idler });
                kapat();
                if (kaynak === 'secim') d.secim.clear();
                await listeDegisti(r.cikan + ' dosya listeden çıkarıldı; listede ' + r.toplam + ' dosya kaldı.');
            } catch (x) { bildir(x.message); tamam.disabled = false; }
        });
    }
    async function listeYonet() {
        await listeleriYukle();
        const govdeL = e('div', { sinif: 'i-pencere-ic' });
        const ciz2 = function() {
            bos(govdeL);
            if (!d.listeler.length) { govdeL.appendChild(e('div', { sinif: 'silik', stil: 'padding:12px 0', metin: 'Henüz liste yok. Dosya Listesi sekmesinde süzgeçleyip "Süzgeçtekileri listeye ekle" ile oluşturun.' })); return; }
            d.listeler.forEach(function(l) {
                const ad = e('input', { type: 'text', value: l.ad, 'aria-label': 'Liste adı', stil: 'flex:1 1 180px;min-width:0' });
                const ac = e('input', { type: 'text', value: l.aciklama || '', placeholder: 'Açıklama', 'aria-label': 'Açıklama', stil: 'flex:2 1 220px;min-width:0' });
                const sil = e('button', { sinif: 'i-d kucuk tehlike', type: 'button', metin: 'Sil' });
                sil.addEventListener('click', async function() {
                    if (sil.dataset.onay !== '1') { sil.dataset.onay = '1'; sil.textContent = 'Silinsin mi? (' + l.dosya_sayisi + ' dosya)'; return; }
                    try {
                        await istek('liste_sil', { id: l.id });
                        if (d.secListe === l.id) { listeSecimiYaz(null); d.secim.clear(); }
                        await listeleriYukle(); ciz2(); listeCubuguYenile(); yukle();
                        bildir('"' + l.ad + '" listesi silindi (dosyalar silinmez).');
                    } catch (x) { bildir(x.message); }
                });
                govdeL.appendChild(e('div', { stil: 'display:flex;flex-wrap:wrap;gap:6px;align-items:center;padding:8px 0;border-bottom:1px solid var(--i-kenar)' }, [
                    ad, ac, e('span', { sinif: 'alt', metin: l.dosya_sayisi + ' dosya · ' + l.acik_sayisi + ' açık' }),
                    e('button', { sinif: 'i-d kucuk', type: 'button', metin: 'Kaydet', onclick: async function() {
                        try { await istek('liste', { id: l.id, ad: ad.value, aciklama: ac.value }); await listeleriYukle(); ciz2(); listeCubuguYenile(); bildir('Kaydedildi.'); }
                        catch (x) { bildir(x.message); }
                    } }),
                    e('button', { sinif: 'i-d kucuk', type: 'button', metin: 'Görüntüle', onclick: function() { kapat(); listeSec(l.id); } }),
                    sil]));
            });
        };
        const kapat = kucukPencere('Dosya listeleri', [govdeL, e('div', { stil: 'display:flex;justify-content:flex-end;margin-top:10px' },
            [e('button', { sinif: 'i-d', type: 'button', metin: 'Kapat', onclick: function() { kapat(); } })])], 760);
        ciz2();
    }

    function durumYaz() {
        const el = document.getElementById('cu-icra-durum');
        if (!el) return;
        const o = d.ozet;
        el.textContent = o ? (o.borclu + ' borçlu · ' + o.acik_icra_dosya + ' açık icra dosyası'
            + (o.son_guncelleme ? ' · son güncelleme ' + gunMetni(o.son_guncelleme) + o.son_guncelleme.slice(10) : '')) : '';
    }

    async function yukle() {
        if (!govde) return;
        d.hata = '';
        try {
            if (d.sekme === 'dosyalar') {
                d.dl.liste = await istek('dosyalar?' + dlSorgusu());
                dlCiz();
            } else if (d.sekme === 'kesin') {
                d.ks.liste = await istek('kesinlestirme?' + dlSorgusu());
                ksCiz();
            } else if (d.sekme === 'surec') {
                const sonuc = await Promise.all([istek('ozet'), istek('borclular?' + listeSorgusu())]);
                d.ozet = sonuc[0]; d.liste = sonuc[1];
                durumYaz();
                surecCiz();
            } else if (d.sekme === 'tebligat') {
                d.teb.liste = await istek('tebligatlar?' + tebSorgusu());
                tebCiz();
            } else if (d.sekme === 'sure') {
                sureCiz(await istek('kurallar'));
            } else {
                tatilCiz(await istek('tatiller'));
            }
        } catch (x) {
            d.hata = x.message;
            bos(govde).appendChild(e('div', { sinif: 'i-ana' }, [e('div', { sinif: 'i-hata', metin: d.hata }),
                e('button', { sinif: 'i-d', metin: 'Yeniden dene', onclick: yukle })]));
        }
    }

    function chip(kod, tarih) {
        return e('span', {}, [e('span', { sinif: 'chip', stil: '--r:' + (ASAMA_RENK[kod] || '#8a877e'), metin: ASAMA_KISA[kod] || kod }),
                              tarih ? e('span', { sinif: 'tarih', metin: gunMetni(tarih) }) : null]);
    }

    // ── Süreçler ──
    function surecCiz() {
        bos(govde);
        const ana = e('div', { sinif: 'i-ana' });
        govde.appendChild(ana);
        const o = d.ozet;
        if (o && !o.icra_dosya) {
            ana.appendChild(e('div', { sinif: 'i-bilgi' }, [
                e('div', { stil: 'font-weight:600;color:var(--i-yazi);margin-bottom:4px', metin: 'Büro veritabanında henüz icra dosyası yok.' }),
                'Dosyalar ve tebligatlar UYAP taraması yapıldığında burada görünecek; takip aşaması, kesinleşme ve tebliğ durumu o zaman hesaplanır.',
                e('div', { stil: 'margin-top:10px' }, [gtDugme()])
            ]));
            if (gtGorunsun()) ana.appendChild(gtPaneli());
            return;
        }
        ana.appendChild(e('div', { sinif: 'i-asamalar' }, (o ? o.asamalar : []).map(function(a) {
            return e('button', {
                sinif: 'i-asama' + (d.filtre.asama === a.kod && a.kod ? ' on' : ''), title: a.etiket,
                onclick: function() { d.filtre.asama = d.filtre.asama === a.kod ? '' : a.kod; d.filtre.sayfa = 1; yukle(); }
            }, [e('div', { sinif: 'n', metin: String(a.adet) }), e('div', { sinif: 't', metin: ASAMA_KISA[a.kod] || a.etiket }),
                e('i', { stil: 'background:' + (ASAMA_RENK[a.kod] || '#8a877e') })]);
        })));

        const ara = e('input', { type: 'search', placeholder: 'Dosya no, birim, borçlu ya da alacaklı ara', value: d.filtre.q,
            oninput: function() {
                clearTimeout(araZaman);
                araZaman = setTimeout(function() { d.filtre.q = ara.value; d.filtre.sayfa = 1; yukle(); }, 300);
            } });
        const durumSec = e('select', { 'aria-label': 'Dosya durumu', title: 'UYAP\'taki dosya durumu' },
            DURUMLAR.map(function(x) { const o = e('option', { value: x[0], metin: x[1] }); if (d.filtre.durum === x[0]) o.selected = true; return o; }));
        durumSec.addEventListener('change', function() { d.filtre.durum = durumSec.value; d.filtre.sayfa = 1; yukle(); });
        const tarihKutusu = function(k, ad) {
            const t = e('input', { type: 'date', value: d.filtre[k], title: ad, 'aria-label': ad });
            t.addEventListener('change', function() { d.filtre[k] = t.value; d.filtre.sayfa = 1; yukle(); });
            return t;
        };
        const hesapla = e('button', { sinif: 'i-d i-hesapla', metin: 'Yeniden hesapla' + (d.secim.size ? ' (' + d.secim.size + ' seçili)' : ''),
            title: 'Kesinleşme, tebliğ durumu ve takip aşamasını kayıtlı tebligatlardan yeniden hesaplar (UYAP\'a gitmez). Seçim varsa yalnız seçili dosyalar.',
            onclick: async function() {
                hesapla.disabled = true;
                try {
                    const s = await istek('hesapla', d.secim.size ? { dosyalar: Array.from(d.secim) } : {});
                    bildir(s.guncellenen ? s.guncellenen + ' borçlunun durumu güncellendi.' : 'Değişen durum yok.');
                    yukle();
                } catch (x) { bildir(x.message); hesapla.disabled = false; }
            } });
        ana.appendChild(e('div', { sinif: 'i-arac' }, [ara, mAracDugmesi(ana), mEk(durumSec),
            mEk(e('label', { sinif: 'i-onay', title: 'Açılış tarihi aralığı (iki gün dahil)' },
              ['Açılış', tarihKutusu('acilis_bas', 'Açılış tarihi: başlangıç'), '–', tarihKutusu('acilis_son', 'Açılış tarihi: bitiş')])),
            e('div', { sinif: 'i-bosluk' }), mEk(gtDugme()), mEk(hesapla)]));
        if (gtGorunsun()) ana.appendChild(gtPaneli());
        if (d.filtre.q && document.activeElement !== ara) setTimeout(function() {
            ara.focus(); ara.setSelectionRange(ara.value.length, ara.value.length);
        }, 0);

        seciliCiz(ana);
        ana.appendChild(e('div', { sinif: 'i-suzgecler m-ek' + (dar() ? '' : ' ust') }, (dar() ? TUM_SUZGEC : UST_SUZGEC).map(function(z) { return suzgecKutusu(z, true); })));

        const l = d.liste;
        const suzBaslik = function(z) { return e('th', { sinif: 'suz suz-' + z.k }, [z.baslik || z.ad, dar() ? null : suzgecKutusu(z, false)]); };
        const basliklar = SUZGEC.slice(0, 5).map(suzBaslik).concat([e('th', { metin: 'Takip aşaması' })]).concat(SUZGEC.slice(5).map(suzBaslik));
        const tablo = e('table', { sinif: 'liste' }, [e('thead', {}, [e('tr', {}, basliklar)])]);
        ana.appendChild(secimCubugu(l));
        if (!l || !l.satirlar.length) {
            tablo.appendChild(e('tbody', {}, [e('tr', {}, [e('td', { colspan: '9', sinif: 'silik', stil: 'text-align:center;padding:22px',
                metin: 'Bu ölçütlere uyan dosya yok.' })])]));
            ana.appendChild(e('div', { sinif: 'i-dl-kutu' }, [tablo]));
        } else {
            l.satirlar.forEach(function(s) { tablo.appendChild(borcluGovdesi(s)); });
            ana.appendChild(e('div', { sinif: 'i-dl-kutu' }, [tablo]));
            const son = Math.max(1, Math.ceil(l.toplam / l.boyut));
            const git = function(n) { return function() { d.filtre.sayfa = n; yukle(); }; };
            ana.appendChild(e('div', { sinif: 'i-sayfa' }, [
                e('span', { metin: l.toplam + ' satır (' + (l.dosya_sayisi != null ? l.dosya_sayisi : '?') + ' dosya) · '
                    + ((l.sayfa - 1) * l.boyut + 1) + '–' + Math.min(l.toplam, l.sayfa * l.boyut) + ' gösteriliyor' }),
                son > 1 ? e('span', {}, [
                    e('button', { sinif: 'i-d kucuk', metin: '‹ Önceki', disabled: l.sayfa <= 1, onclick: git(l.sayfa - 1) }), ' ',
                    l.sayfa + ' / ' + son, ' ',
                    e('button', { sinif: 'i-d kucuk', metin: 'Sonraki ›', disabled: l.sayfa >= son, onclick: git(l.sayfa + 1) })
                ]) : null
            ]));
        }
        ana.addEventListener('scroll', acilirKaydir);
        // Yeniden çizimden sonra: yazılmakta olan başlık kutusu odağını ve açık listesini korur.
        if (d.kutu) {
            const kutu = kok.querySelector('.i-kutu-s[data-k="' + d.kutu + '"] input');
            if (kutu) { kutu.focus(); kutu.setSelectionRange(kutu.value.length, kutu.value.length); acilirCiz(); }
            else acilirKapat();
        }
        if (d.secili) panelCiz();
    }

    // ── Başlık süzgeçleri ("yaz ya da seç") ──
    // Süzgeç kutuları iki sekmede ortak: etkin sekmenin süzgeci (F), listesi (L), süzgeç tanımları (SZ).
    function tebSekmesi() { return d.sekme === 'tebligat'; }
    function F() { return tebSekmesi() ? d.teb.filtre : d.filtre; }
    function L() { return tebSekmesi() ? d.teb.liste : d.liste; }
    function SZ() { return tebSekmesi() ? TEB_SUZGEC : TUM_SUZGEC; }
    // Dosya paneli açılıp kapanınca liste baştan çizilir; kaydırma yeri korunur (kullanıcı bulgusu 21.11.2026: satıra
    // tıklayınca sayfa en üste çıkıyordu).
    function aktifCiz() {
        const eski = govde && govde.querySelector('.i-ana');
        const yer = eski ? eski.scrollTop : 0;
        const kutu = govde && govde.querySelector('.i-dl-kutu');
        const yatay = kutu ? kutu.scrollLeft : 0, dikey = kutu ? kutu.scrollTop : 0;
        const eskiPanel = govde && govde.querySelector('.i-panel');
        const panelYer = eskiPanel ? eskiPanel.scrollTop : 0;
        if (d.sekme === 'dosyalar') dlCiz(); else if (d.sekme === 'kesin') ksCiz(); else if (tebSekmesi()) tebCiz(); else surecCiz();
        const yeni = govde && govde.querySelector('.i-ana');
        if (yeni) yeni.scrollTop = yer;
        const yeniKutu = govde && govde.querySelector('.i-dl-kutu');
        if (yeniKutu) { yeniKutu.scrollLeft = yatay; yeniKutu.scrollTop = dikey; }
        const yeniPanel = govde && govde.querySelector('.i-panel');
        if (yeniPanel && panelYer) yeniPanel.scrollTop = panelYer;
    }
    // Tarih aralığı süzgeçleri (sekmede olanlar etiket olarak görünür).
    const ARALIKLAR = [['acilis', 'Açılış'], ['evrak', 'Evrak tarihi'], ['islem', 'Tebliğ / iade tarihi']];
    function secenekler(k) { return (L() && L().secenekler && L().secenekler[k]) || []; }
    function suzgecBul(k) { return SZ().find(function(z) { return z.k === k; }); }
    // Seçili süzgecin ekranda görünen karşılığı (listeden seçildiyse etiketi, yazıldıysa yazının kendisi).
    // Listeden seçilen değerler (birden çok seçilebilir; süzgeçte satır sonuyla ayrılır). Yazıyla süzülüyorsa boş.
    function seciliDegerler(z) {
        const v = F()[z.k];
        if (!v || haricMi(z)) return [];
        if (z.kodlu) return v.split('\n');
        return v.charAt(0) === '=' ? v.slice(1).split('\n') : [];
    }
    // "Tümünü seç" sonra bazılarını kaldır: süzgeç "!" + hariç tutulanlar ("!" tek başına: hepsi; yalnız boş değer: "!\n").
    function haricMi(z) { return String(F()[z.k] || '').charAt(0) === '!'; }
    function haricDegerler(z) {
        if (!haricMi(z)) return [];
        const r = F()[z.k].slice(1);
        return r === '' ? [] : r === '\n' ? [''] : r.split('\n');
    }
    function haricYaz(z, liste) { return '!' + (liste.length === 1 && liste[0] === '' ? '\n' : liste.join('\n')); }
    // Listeden mi seçildi (tam değerler ya da hariç listesi); değilse yazıyla süzülüyor.
    function listedenMi(z) { const v = F()[z.k]; return !!v && (z.kodlu || v.charAt(0) === '=' || v.charAt(0) === '!'); }
    function tumunuSec(z) {
        clearTimeout(kutuZaman);
        F()[z.k] = '!';
        F().sayfa = 1;
        yukle();
    }
    function degerEtiketi(z, deger) {
        const sec = secenekler(z.k).find(function(o) { return o.deger === deger; });
        return sec ? sec.etiket : (deger || '(boş)');
    }
    function suzgecEtiketi(z) {
        const v = F()[z.k];
        if (!v) return '';
        if (haricMi(z)) { const h = haricDegerler(z); return h.length ? 'Tümü (' + h.length + ' hariç)' : 'Tümü'; }
        if (!z.kodlu && v.charAt(0) !== '=') return v;
        const s = seciliDegerler(z);
        return degerEtiketi(z, s[0]) + (s.length > 1 ? ' +' + (s.length - 1) : '');
    }
    // Açılır listede bir seçeneği seçer / bırakır; liste açık kalır, başka seçenekler de işaretlenebilir.
    function secimDegistir(z, deger) {
        if (haricMi(z)) {
            const h = haricDegerler(z);
            const j = h.indexOf(deger);
            if (j >= 0) h.splice(j, 1); else h.push(deger);
            clearTimeout(kutuZaman);
            F()[z.k] = haricYaz(z, h);
            F().sayfa = 1;
            yukle();
            return;
        }
        const s = seciliDegerler(z);
        const i = s.indexOf(deger);
        if (i >= 0) s.splice(i, 1); else s.push(deger);
        clearTimeout(kutuZaman);
        F()[z.k] = s.length ? (z.kodlu ? '' : '=') + s.join('\n') : '';
        F().sayfa = 1;
        yukle();
    }
    function suzgecUygula(k, deger) {
        clearTimeout(kutuZaman);
        F()[k] = deger;
        F().sayfa = 1;
        yukle();
    }
    function suzgecKutusu(z, yerTutucuAd) {
        const dolu = !!F()[z.k];
        const acik = d.kutu === z.k;
        const giris = e('input', { type: 'text', autocomplete: 'off', spellcheck: 'false',
            // Liste açıkken (seçim sonrası yeniden çizim) kutu boş, seçim yer tutucuda: yazı seçimin sonuna eklenmez.
            placeholder: acik && dolu ? suzgecEtiketi(z) : (yerTutucuAd ? z.ad : (z.kodlu ? 'Seçin' : 'Yazın ya da seçin')),
            'aria-label': z.ad + ' süzgeci', title: z.ad + ': ' + (z.kodlu ? 'listeden seçin' : 'yazın (içinde geçenler) ya da listeden seçin; birden çok seçilebilir'),
            value: acik ? (d.kutuYazildi ? d.kutuMetin : '') : suzgecEtiketi(z) });
        giris.addEventListener('focus', function() {
            if (d.kutu !== z.k) { d.kutu = z.k; d.kutuMetin = ''; d.kutuYazildi = false; giris.select(); }
            acilirCiz();
        });
        giris.addEventListener('input', function() {
            d.kutu = z.k; d.kutuMetin = giris.value; d.kutuYazildi = true;
            acilirCiz();
            // Listeden seçim yapılmışsa yazı yalnız açılır listeyi daraltır (seçimler kaybolmasın).
            if (z.kodlu || listedenMi(z)) return;
            // Yazarak süzme: içinde geçenler (kısa bir beklemeyle, her tuşta sorgu gitmesin).
            clearTimeout(kutuZaman);
            kutuZaman = setTimeout(function() { F()[z.k] = giris.value.trim(); F().sayfa = 1; yukle(); }, 350);
        });
        giris.addEventListener('keydown', function(ev) {
            if (ev.key === 'Escape') { ev.preventDefault(); kutuBirak(); giris.blur(); aktifCiz(); return; }
            if (ev.key === 'ArrowDown' || ev.key === 'ArrowUp') { ev.preventDefault(); imlecOynat(ev.key === 'ArrowDown' ? 1 : -1); return; }
            if (ev.key !== 'Enter') return;
            ev.preventDefault();
            const imlec = kok.querySelector('.i-acilir .sec.imlec');
            if (imlec) { imlec.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })); return; }
            if (z.kodlu) {
                const ilk = gorunenSecenekler(z)[0];
                if (ilk) secimDegistir(z, ilk.deger);
            } else { const yazi = giris.value.trim(); kutuBirak(); suzgecUygula(z.k, yazi); }
        });
        giris.addEventListener('blur', function() {
            // Liste satırına basış mousedown ile işlenir; burada yalnız kutu bırakılır.
            setTimeout(function() {
                // Liste yeniden çizildiyse bu kutu artık sayfada değil (yenisi odağı aldı): dokunma.
                if (!giris.isConnected || d.kutu !== z.k || document.activeElement === giris) return;
                const yazim = d.kutuYazildi && !z.kodlu;
                kutuBirak();
                if (!yazim && kok) giris.value = suzgecEtiketi(z);
            }, 120);
        });
        return e('div', { sinif: 'i-kutu-s' + (dolu ? ' dolu' : ''), 'data-k': z.k, onclick: function(ev) { ev.stopPropagation(); } }, [
            giris,
            dolu ? e('button', { sinif: 'sil', type: 'button', metin: '✕', title: z.ad + ' süzgecini kaldır', 'aria-label': z.ad + ' süzgecini kaldır',
                onmousedown: function(ev) { ev.preventDefault(); kutuBirak(); suzgecUygula(z.k, ''); } }) : null,
            e('button', { sinif: 'ok', type: 'button', metin: '▾', tabindex: '-1', title: 'Seçenekleri göster', 'aria-label': z.ad + ' seçenekleri',
                onmousedown: function(ev) {
                    ev.preventDefault();
                    if (d.kutu === z.k && kok.querySelector('.i-acilir')) { kutuBirak(); giris.blur(); }
                    else { d.kutu = null; giris.focus(); }
                } })
        ]);
    }
    function kutuBirak() { d.kutu = null; d.kutuMetin = ''; d.kutuYazildi = false; acilirKapat(); }
    // Liste kaydırılınca açık seçenek listesi kutunun yeni yerine taşınır; kutu görünür alandan çıkınca kapanır
    // (geniş tabloda sağdaki kutuya tıklayınca tarayıcı yana kaydırıyor; eskiden liste hemen kapanıyordu).
    let kaydirZaman = 0;
    function acilirKaydir(ev) {
        if (!kok || !kok.querySelector('.i-acilir') || kaydirZaman) return;
        const ana = ev.currentTarget;
        kaydirZaman = requestAnimationFrame(function() {
            kaydirZaman = 0;
            const kap = d.kutu && kok.querySelector('.i-kutu-s[data-k="' + d.kutu + '"]');
            if (!kap) { acilirKapat(); return; }
            const r = kap.getBoundingClientRect(), a = ana.getBoundingClientRect();
            if (r.bottom < a.top || r.top > a.bottom || r.right < a.left || r.left > a.right) acilirKapat(); else acilirCiz();
        });
    }
    function acilirKapat() { if (!kok) return; const a = kok.querySelector('.i-acilir'); if (a) a.remove(); }
    // Kutuya yazılan kelimelerin hepsi etikette geçiyor mu (açılır listeyi daraltmak için).
    function yaziyaUyar(etiket) {
        const aranan = d.kutuYazildi ? trKucuk(d.kutuMetin).split(/\s+/).filter(Boolean) : [];
        const m = trKucuk(etiket);
        return aranan.every(function(a) { return m.indexOf(a) >= 0; });
    }
    function gorunenSecenekler(z) {
        return secenekler(z.k).filter(function(o) { return yaziyaUyar(o.etiket); });
    }
    function imlecOynat(yon) {
        const hepsi = Array.from(kok.querySelectorAll('.i-acilir .sec'));
        if (!hepsi.length) return;
        let i = hepsi.findIndex(function(x) { return x.classList.contains('imlec'); });
        if (i >= 0) hepsi[i].classList.remove('imlec');
        i = (i + yon + hepsi.length + (i < 0 && yon < 0 ? 1 : 0)) % hepsi.length;
        hepsi[i].classList.add('imlec');
        hepsi[i].scrollIntoView({ block: 'nearest' });
    }
    // Açık kutunun seçenek listesi: adetleriyle, yazılan metne göre daralır. Sayfanın üstünde sabit konumda
    // (tablonun köşe yuvarlaması taşanı kestiği için başlığın içine konmaz).
    function acilirCiz() {
        acilirKapat();
        const z = suzgecBul(d.kutu);
        const kap = z && kok.querySelector('.i-kutu-s[data-k="' + z.k + '"]');
        if (!kap) return;
        const secili = F()[z.k];
        const haric = haricMi(z);
        const seciler = haric ? haricDegerler(z) : seciliDegerler(z);
        const liste = gorunenSecenekler(z);
        // Seçilip (ya da hariç tutulup) listede olmayan değerler (sınırın dışında kalan ya da artık adedi olmayan) de görünsün.
        seciler.forEach(function(s) {
            const etiket = degerEtiketi(z, s);
            if (yaziyaUyar(etiket) && !liste.some(function(o) { return o.deger === s; })) liste.unshift({ deger: s, etiket: etiket, adet: null });
        });
        const kutu = e('div', { sinif: 'i-acilir', role: 'listbox', 'aria-label': z.ad, 'aria-multiselectable': 'true' });
        if (secili) kutu.appendChild(e('div', { sinif: 'sec', role: 'option',
            onmousedown: function(ev) { ev.preventDefault(); kutuBirak(); suzgecUygula(z.k, ''); } },
            [e('span', { sinif: 'ad', metin: 'Hepsi (süzgeci kaldır)' })]));
        // Tümünü seç: hepsi işaretlenir, istenmeyenler tek tek kaldırılır (süzgeç "hariç" olur).
        // Kutuya yazı yazılırken gösterilmez (yazılan metinle "hepsi" çelişir).
        const yaziyor = d.kutuYazildi && d.kutuMetin.trim();
        if (secenekler(z.k).length > 1 && !(haric && !seciler.length) && !yaziyor) kutu.appendChild(e('div', { sinif: 'sec', role: 'option',
            title: 'Hepsini işaretler; istemediklerinizin işaretini kaldırın',
            onmousedown: function(ev) { ev.preventDefault(); tumunuSec(z); } },
            [e('span', { sinif: 'kt', metin: '' }), e('span', { sinif: 'ad', metin: 'Tümünü seç' })]));
        liste.forEach(function(o) {
            const on = haric ? seciler.indexOf(o.deger) < 0 : seciler.indexOf(o.deger) >= 0;
            kutu.appendChild(e('div', { sinif: 'sec' + (on ? ' on' : ''), role: 'option', 'aria-selected': on ? 'true' : 'false',
                onmousedown: function(ev) { ev.preventDefault(); secimDegistir(z, o.deger); } },
                [e('span', { sinif: 'kt', metin: on ? '✓' : '' }), e('span', { sinif: 'ad', metin: o.etiket }),
                 e('span', { metin: o.adet === null ? '' : String(o.adet) })]));
        });
        if (!liste.length) kutu.appendChild(e('div', { sinif: 'yok', metin: secenekler(z.k).length ? 'Yazdığınıza uyan seçenek yok.' : 'Bu ölçütlerde seçenek yok.' }));
        const sinir = L() && L().secenek_siniri;
        if (sinir && secenekler(z.k).length >= sinir) kutu.appendChild(e('div', { sinif: 'yok', metin: 'En çok kullanılan ' + sinir + ' seçenek gösteriliyor; diğerleri için yazın.' }));
        kok.appendChild(kutu);
        const r = kap.getBoundingClientRect();
        const g = kutu.getBoundingClientRect();
        kutu.style.left = Math.max(8, Math.min(r.left, window.innerWidth - g.width - 8)) + 'px';
        const altta = window.innerHeight - r.bottom - 8;
        if (altta < 160 && r.top > altta) { kutu.style.maxHeight = Math.min(320, r.top - 8) + 'px'; kutu.style.bottom = (window.innerHeight - r.top + 2) + 'px'; }
        else { kutu.style.maxHeight = Math.min(320, altta) + 'px'; kutu.style.top = (r.bottom + 2) + 'px'; }
        kutu.style.minWidth = Math.max(220, r.width) + 'px';
    }

    // Seçili süzgeçler: tek satırda etiketler; tıklayınca o süzgeç kalkar.
    function seciliCiz(ana) {
        const etiketler = [];
        if (F().asama) etiketler.push(['Aşama', ASAMA_KISA[F().asama] || F().asama, function() { F().asama = ''; }]);
        ARALIKLAR.forEach(function(a) {
            const f = F(), b = f[a[0] + '_bas'], s = f[a[0] + '_son'];
            if (b || s) etiketler.push([a[1], (b ? gunMetni(b) : '…') + ' – ' + (s ? gunMetni(s) : '…'),
                function() { f[a[0] + '_bas'] = ''; f[a[0] + '_son'] = ''; }]);
        });
        SZ().forEach(function(z) {
            const v = F()[z.k];
            if (!v) return;
            const haric = haricMi(z);
            const yazi = !z.kodlu && !haric && v.charAt(0) !== '=';
            const adlar = yazi ? [] : (haric ? haricDegerler(z) : seciliDegerler(z)).map(function(s) { return degerEtiketi(z, s); });
            const metin = adlar.join(', ');
            const kisa = metin.length > 70 ? adlar[0] + ' ve ' + (adlar.length - 1) + ' seçim daha' : metin;
            etiketler.push([z.ad, yazi ? '“' + v + '” geçen' : haric ? (adlar.length ? 'tümü, hariç: ' + kisa : 'tümü') : kisa,
                            function() { F()[z.k] = ''; }]);
        });
        if (!etiketler.length) return;
        const kaldir = function(fn) { return function() { fn(); kutuBirak(); F().sayfa = 1; yukle(); }; };
        ana.appendChild(e('div', { sinif: 'i-secili' }, [e('span', { metin: 'Süzgeç:' })].concat(etiketler.map(function(x) {
            return e('button', { sinif: 'et', type: 'button', title: x[0] + ' süzgecini kaldır', onclick: kaldir(x[2]) },
                [e('span', { metin: x[0] + ':' }), e('b', { metin: x[1] }), e('i', { metin: '✕' })]);
        })).concat(etiketler.length > 1 ? [e('button', { sinif: 'i-dahasi', type: 'button', metin: 'Hepsini kaldır',
            onclick: kaldir(function() {
                const f = F();
                if (f.asama) f.asama = '';
                ARALIKLAR.forEach(function(a) { if (a[0] + '_bas' in f) { f[a[0] + '_bas'] = ''; f[a[0] + '_son'] = ''; } });
                SZ().forEach(function(z) { F()[z.k] = ''; });
            }) })] : [])));
    }

    // ── Dosya seçimi: satırlardaki onay kutuları + "Tümünü seç" (süzgeçteki bütün dosyalar, bütün sayfalar) ──
    function secimYaz() {
        const n = d.secim.size;
        const sayac = kok && kok.querySelector('.i-secim-sayi');
        if (sayac) sayac.textContent = n ? n + ' dosya seçili' : 'Seçili dosya yok';
        const birak = kok && kok.querySelector('.i-secim-birak');
        if (birak) birak.disabled = !n;
        const hes = kok && kok.querySelector('.i-hesapla');
        if (hes) hes.textContent = 'Yeniden hesapla' + (n ? ' (' + n + ' seçili)' : '');
        if (kok) kok.querySelectorAll('input.i-dosya-sec').forEach(function(k) { k.checked = d.secim.has(Number(k.getAttribute('data-dosya'))); });
        if (gtGorunsun() && !gtSuruyor()) gtCiz();
    }
    function secimCubugu(l) {
        const toplam = l && l.dosya_sayisi != null ? l.dosya_sayisi : 0;
        const tumu = e('button', { sinif: 'i-d kucuk', type: 'button', metin: 'Tümünü seç (' + toplam + ' dosya)', disabled: !toplam,
            title: 'Süzgeçteki bütün dosyaları seçer (bütün sayfalar); önceki seçime eklenir',
            onclick: async function() {
                tumu.disabled = true;
                try {
                    const idler = await istek('dosya_kimlikleri?' + listeSorgusu());
                    idler.forEach(function(x) { d.secim.add(x); });
                    secimYaz();
                } catch (x) { bildir(x.message); }
                tumu.disabled = false;
            } });
        return e('div', { sinif: 'i-secim', stil: 'display:flex;gap:8px;align-items:center;flex-wrap:wrap;margin:6px 0' }, [
            tumu,
            e('button', { sinif: 'i-d kucuk i-secim-birak', type: 'button', metin: 'Tümünü bırak', disabled: !d.secim.size,
                onclick: function() { d.secim.clear(); secimYaz(); } }),
            e('span', { sinif: 'i-secim-sayi alt', metin: d.secim.size ? d.secim.size + ' dosya seçili' : 'Seçili dosya yok' }),
            e('span', { sinif: 'alt', metin: '· "Toplu tarama" seçim varsa yalnız seçili, yoksa süzgeçteki dosyalara işler.' })
        ]);
    }

    // ── Borçlu satırı: künye + borçlunun bütün tebligatları (tebligat başına bir alt satır) ──
    function tebligatHucreleri(t, suzgecVar, ilk) {
        const sol = suzgecVar && !t.uyan ? ' sol' : '';
        const sinif = 'tb' + sol + (ilk ? ' t1' : '');
        const alt1 = [t.evrak_tarihi, t.elektronik ? 'E-tebligat' : (t.barkod || 'barkod yok')].filter(Boolean).join(' · ');
        const alt2 = [t.son_islem_tarihi, t.teslim_sekli].filter(Boolean).join(' · ');
        return [
            e('td', { sinif: sinif + ' tb0' }, [e('div', { metin: t.tur || 'Tebligat' }), e('div', { sinif: 'alt', metin: alt1 })]),
            e('td', { sinif: sinif }, [t.ptt_durumu ? e('div', { sinif: 'uzun', title: t.ptt_durumu, metin: t.ptt_durumu })
                                                     : e('div', { sinif: 'silik', metin: 'Henüz sorgulanmadı' }),
                                       alt2 ? e('div', { sinif: 'alt', metin: alt2 }) : null]),
            e('td', { sinif: sinif }, [mazbataHucresi(t)])
        ];
    }
    // Kesinleşme hücresi (tebligat başına). Tabloda borçludan hemen sonra, Takip aşamasından önce durur.
    function kesinlesmeHucresi(t, suzgecVar, ilk) {
        const sinif = 'tb kes' + (suzgecVar && !t.uyan ? ' sol' : '') + (ilk ? ' t1' : '');
        return e('td', { sinif: sinif }, [t.kesinlesme ? e('span', { metin: t.kesinlesme })
            : e('span', { sinif: 'silik', metin: '—', title: t.kesinlesme_neden || (t.kesinlesme_yok ? 'Bu tebligat türünde kesinleşme yok' : 'Henüz hesaplanmadı') })]);
    }
    function borcluGovdesi(s) {
        const suzgecVar = !!(d.filtre.tur || d.filtre.teblig || d.filtre.mazbata);
        // Süzgece uyan tebligatlar öne alınır; ardından kesinleşme taşıyanlar (ödeme / tahliye emri): Kesinleşme
        // sütunu tebligat başınadır, sonradan gelen banka yazıları ödeme emrini "+N tebligat daha" altına itip
        // sütunu boş gösteriyordu (kullanıcı bulgusu 20.11.2026, İzmir 10. İcra 2026/7574). Sıra içinde yeniden eskiye kalır.
        const hepsi = (s.tebligatlar || []).slice().sort(function(a, b) {
            return (suzgecVar ? (b.uyan ? 1 : 0) - (a.uyan ? 1 : 0) : 0) || (b.kesinlesme || b.kesinlesme_neden ? 1 : 0) - (a.kesinlesme || a.kesinlesme_neden ? 1 : 0);
        });
        const tumu = !!d.tumTebligat[s.bag];
        const gorunen = tumu ? hepsi : hepsi.slice(0, TEBLIGAT_GORUNEN);
        const fazla = hepsi.length - TEBLIGAT_GORUNEN;
        const satirSayisi = Math.max(1, gorunen.length) + (fazla > 0 ? 1 : 0);
        const secKutu = e('input', { type: 'checkbox', sinif: 'i-dosya-sec', 'data-dosya': String(s.dosya), checked: d.secim.has(s.dosya),
            'aria-label': 'Dosyayı seç: ' + s.birim + ' ' + s.dosya_no, stil: 'margin:2px 8px 0 0;flex:none',
            onclick: function(ev) { ev.stopPropagation(); } });
        secKutu.addEventListener('change', function() {
            if (secKutu.checked) d.secim.add(s.dosya); else d.secim.delete(s.dosya);
            secimYaz();
        });
        const kunye = [
            e('td', { rowspan: String(satirSayisi) }, [e('div', { stil: 'display:flex;align-items:flex-start' }, [secKutu, e('div', { metin: s.birim })])]),
            e('td', { rowspan: String(satirSayisi) }, [e('div', { sinif: 'dno', metin: s.dosya_no }),
                e('div', { sinif: 'alt', metin: [s.dosya_tur, s.acilis_tarihi ? 'açılış ' + gunMetni(s.acilis_tarihi) : ''].filter(Boolean).join(' · ') }),
                s.acik ? null : e('div', { sinif: 'alt', metin: s.dosya_durumu || 'Kapalı' })]),
            e('td', { rowspan: String(satirSayisi), metin: s.alacakli || '—' }),
            e('td', { rowspan: String(satirSayisi) }, s.bag == null ? [e('span', { sinif: 'silik', metin: 'Borçlu kaydı yok' })]
                : [s.borclu, s.uyapta_gorulmedi ? e('div', { sinif: 'uyari', metin: 'UYAP\'ta görülmedi' }) : null])
        ];
        // Takip aşaması dosya boyunca tek hücre; Kesinleşme (tebligat başına) ondan önce gelir. Sonraki satırlarda
        // tarayıcı rowspan'lı sütunları atlar, Kesinleşme yine 5. sütuna düşer.
        const asamaHucre = e('td', { rowspan: String(satirSayisi) }, s.bag == null ? [] : [chip(s.asama, s.asama_tarihi)]);
        const govdeEl = e('tbody', { sinif: 'tik' + (d.secili && d.secili.id === s.dosya ? ' sec' : ''), onclick: function() { dosyaAc(s.dosya); } });
        if (!gorunen.length) {
            // Tebligatsız borçluda itiraz / durdurma gibi borçlu kesinleşmesi yine görünür.
            govdeEl.appendChild(e('tr', {}, kunye.concat([e('td', { sinif: 'tb kes t1', metin: s.kesinlesme || '—' }), asamaHucre,
                e('td', { colspan: '3', sinif: 'tb t1 tb0 silik',
                    metin: s.bag == null ? 'Dosyanın tarafları henüz taranmadı' : 'Bu borçluya gönderilmiş tebligat kaydı yok' })])));
        } else {
            gorunen.forEach(function(t, i) {
                const kes = kesinlesmeHucresi(t, suzgecVar, i === 0);
                govdeEl.appendChild(e('tr', { sinif: i ? 'ek' : '' },
                    (i ? [kes] : kunye.concat([kes, asamaHucre])).concat(tebligatHucreleri(t, suzgecVar, i === 0))));
            });
        }
        if (fazla > 0) {
            // Kesinleşme sütunu (5.) boş, düğme tebligat sütunlarını (7–9) kaplar; arada Takip aşamasının rowspan'ı var.
            govdeEl.appendChild(e('tr', { sinif: 'ek' }, [e('td', { sinif: 'tb kes' }), e('td', { colspan: '3', sinif: 'tb' }, [e('button', { sinif: 'i-dahasi', type: 'button',
                metin: tumu ? 'Daha az göster' : '+' + fazla + ' tebligat daha',
                title: tumu ? '' : 'Bu borçlunun bütün tebligatlarını göster (' + hepsi.length + ')',
                onclick: function(ev) { ev.stopPropagation(); if (tumu) delete d.tumTebligat[s.bag]; else d.tumTebligat[s.bag] = true; surecCiz(); } })])]));
        }
        return govdeEl;
    }

    // ── UYAP'tan güncelleme ──
    // İstekler ofis bilgisayarından, sırayla ve yavaş hızla UYAP'a gider (icra_guncelle.rs). Sayfa yalnız başlatır,
    // izler, durdurur ve doğrulama (captcha) çıkarsa kullanıcıya çözdürür.
    const GT_UC = '/__uyap_agent__/icra_guncelle';
    const GT_SON = ['bitti', 'hata', 'iptal'];
    const GT_ASAMA = { liste: 'Dosya listesi alınıyor', arama: 'Seçili dosyalar UYAP\'ta aranıyor', kunye: 'Künyeler yazılıyor', taraf: 'Taraflar ve borçlu kimlikleri alınıyor',
                       kapanan: 'Kapanan dosyalar kontrol ediliyor', tebligat: 'Tebligatlar alınıyor (evrak listesi, tebligat belgesi, PTT)',
                       ptt: 'Barkodlar PTT\'de sorgulanıyor', takip: 'Takip türü bilgileri alınıyor', takip_listesi: 'UYAP takip türü listesi alınıyor',
                       hesap: 'Durumlar hesaplanıyor', bitti: 'Bitti',
                       kesinlesme: 'Kesinleşme UYAP\'tan okunuyor', talep: 'Kesinleştirme talepleri hazırlanıyor, e-imzalanıyor ve gönderiliyor',
                       mernis: 'Mernis adresleri sorgulanıyor ve ilk tebligat adresiyle karşılaştırılıyor',
                       yeniden_teblig: 'Yeniden tebliğ talepleri hazırlanıyor, e-imzalanıyor ve gönderiliyor (ücretli)' };
    const GT_TUR = { liste: 'Dosya listesi', taraf: 'Taraf bilgileri', tebligat: 'Tebligat bilgileri', ptt: 'PTT durumları',
                     takip: 'Takip türü bilgisi', takip_listesi: 'Takip türü listesi', hepsi: 'Tüm veriler',
                     kesinlesme: 'Kesinleşme bilgisi', kesinlestirme_talebi: 'Kesinleştirme talepleri',
                     mernis: 'Mernis sorgusu', yeniden_teblig: 'Yeniden tebliğ talepleri' };
    const GT_KESIN = ['kesinlesme', 'kesinlestirme_talebi', 'mernis', 'yeniden_teblig'];
    let gtZaman = null, tsYukleniyor = null;
    function gtSuruyor() { return !!(d.gt && GT_SON.indexOf(d.gt.durum) < 0); }
    function gtGorunsun() { return d.gtAcik || gtSuruyor(); }
    async function gtIstek(yol, govde) {
        let r;
        try {
            r = await fetch(GT_UC + yol, govde === undefined
                ? { cache: 'no-store', credentials: 'same-origin' }
                : { method: 'POST', cache: 'no-store', credentials: 'same-origin',
                    headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(govde) });
        } catch (x) { throw new Error('Ofis bilgisayarına ulaşılamadı.'); }
        let j = null;
        try { j = await r.json(); } catch (x) {}
        if (!j) throw new Error('Ofis bilgisayarından beklenmeyen yanıt (' + r.status + '). Program güncel olmayabilir.');
        if (j.error || j.ok === false) throw new Error(j.error || 'İşlem yapılamadı.');
        return j.tarama || null;
    }
    // Tarama sürerken yazılan dosyalar listede görünsün: sayaçlar değişince liste sessizce yenilenir
    // (en çok 10 sn'de bir). Kullanıcı başlık kutusunda yazıyor ya da bir alana odaklanmışsa beklenir;
    // kaydırma yeri korunur.
    const CANLI_ARA_MS = 10000;
    let canliIz = '', canliSon = 0, canliSuruyor = false;
    async function canliYenile(t) {
        const iz = [t.yazilan, t.taraf_cekilen, t.kapandi, t.yeni_dosya, t.teb_yazilan].join('/');
        if (iz === canliIz || canliSuruyor || t.captcha || !govde || d.sekme !== 'surec' || d.kutu) return;
        if (Date.now() - canliSon < CANLI_ARA_MS) return;
        const odak = document.activeElement;
        if (odak && kok && kok.contains(odak) && /^(INPUT|SELECT|TEXTAREA)$/.test(odak.tagName)) return;
        canliSuruyor = true;
        try {
            const sonuc = await Promise.all([istek('ozet'), istek('borclular?' + listeSorgusu())]);
            if (!govde || d.sekme !== 'surec' || d.kutu) return;
            const ana = govde.querySelector('.i-ana');
            const yer = ana ? ana.scrollTop : 0;
            d.ozet = sonuc[0]; d.liste = sonuc[1];
            durumYaz();
            surecCiz();
            const yeniAna = govde.querySelector('.i-ana');
            if (yeniAna) yeniAna.scrollTop = yer;
            canliIz = iz;
        } catch (x) {
            // sessiz: bir sonraki yoklamada yeniden denenir
        } finally {
            canliSon = Date.now(); canliSuruyor = false;
        }
    }
    // Süren taramayı izler (2 sn'de bir); bitince liste ve özet yenilenir.
    async function gtYokla(ilk) {
        clearTimeout(gtZaman);
        let t;
        try { t = await gtIstek(''); }
        catch (x) { if (!ilk && gtSuruyor()) gtZaman = setTimeout(gtYokla, 10000); return; }
        const onceSuruyordu = gtSuruyor();
        d.gt = t;
        if (gtSuruyor()) gtZaman = setTimeout(gtYokla, 2000);
        if (onceSuruyordu && !gtSuruyor()) { d.gtAcik = true; canliIz = ''; yukle(); if (d.secili) dosyaAc(d.secili.id); return; }
        gtCiz();
        if (gtSuruyor()) canliYenile(t);
    }
    function gtCiz() {
        if (!kok) return;
        const dugme = kok.querySelector('.i-gt-dugme');
        if (dugme) dugme.replaceWith(gtDugme());
        const eski = kok.querySelector('.i-gt');
        // Doğrulama kutusu açıkken yeniden çizilmez (kullanıcının çözdüğü kutu kaybolmasın).
        if (eski && d.gt && d.gt.captcha && eski.getAttribute('data-captcha') === String(d.gt.captcha.n)) return;
        if (!gtGorunsun()) { if (eski) eski.remove(); return; }
        const yeni = gtPaneli();
        if (eski) { eski.replaceWith(yeni); return; }
        const arac = kok.querySelector('.i-arac') || kok.querySelector('.i-bilgi');
        if (arac) arac.after(yeni);
    }
    function gtDugme() {
        const t = d.gt;
        const metin = gtSuruyor()
            ? 'Taranıyor' + (t.toplam ? ' · ' + Math.min(100, Math.round(t.i * 100 / t.toplam)) + '%' : '…')
            : 'Toplu tarama';
        return e('button', { sinif: 'i-d i-gt-dugme' + (gtSuruyor() ? ' ana' : ''), type: 'button', metin: metin,
            title: 'UYAP\'taki açık icra dosyalarını büro veritabanına alır: dosya listesi, taraf ya da tebligat bilgileri',
            onclick: function() { d.gtAcik = gtSuruyor() ? true : !d.gtAcik; gtCiz(); if (d.gtAcik) gtYokla(true); } });
    }
    function mesaiMi() {
        const s = new Date(), gun = s.getDay(), dk = s.getHours() * 60 + s.getMinutes();
        return gun >= 1 && gun <= 5 && dk >= 8 * 60 + 30 && dk < 18 * 60;
    }
    function sureMetni(sn) {
        if (sn == null) return '';
        if (sn < 60) return '1 dakikadan az';
        const dk = Math.round(sn / 60);
        return dk < 60 ? '≈ ' + dk + ' dk' : '≈ ' + Math.floor(dk / 60) + ' sa ' + (dk % 60) + ' dk';
    }
    function gtPaneli() {
        const t = d.gt;
        const kutu = e('div', { sinif: 'i-gt', role: 'region', 'aria-label': 'Toplu tarama' });
        kutu.appendChild(e('div', { sinif: 'i-gt-ust' }, [e('b', { metin: 'Toplu tarama (UYAP)' }), e('div', { sinif: 'i-bosluk' }),
            gtSuruyor() ? null : e('button', { sinif: 'i-d kucuk', type: 'button', metin: 'Kapat', onclick: function() { d.gtAcik = false; gtCiz(); } })]));
        if (gtSuruyor()) { gtSurenCiz(kutu, t); return kutu; }
        if (t) gtSonucCiz(kutu, t);
        // Dosya Listesi sekmesinde kapsam ve düğmeler sekmenin kendi araç çubuğunda; burada yalnız sonuç.
        if (d.sekme === 'surec' || d.sekme === 'tebligat') gtFormCiz(kutu, t);
        return kutu;
    }
    function gtTurAdi(t) { return (t && GT_TUR[t.mod]) || GT_TUR.liste; }
    function gtSurenCiz(kutu, t) {
        kutu.appendChild(e('div', { stil: 'font-weight:600', metin: gtTurAdi(t) + ' güncelleniyor' + (t.kapsam ? ' — ' + t.kapsam : '') }));
        kutu.appendChild(e('div', { metin: GT_ASAMA[t.asama] || t.asama }));
        kutu.appendChild(e('div', { sinif: 'i-gt-not', metin: t.mesaj || '' }));
        if (t.toplam) {
            kutu.appendChild(e('div', { sinif: 'i-gt-cubuk', role: 'progressbar', 'aria-valuemin': '0', 'aria-valuemax': String(t.toplam), 'aria-valuenow': String(t.i) },
                [e('i', { stil: 'width:' + Math.min(100, t.i * 100 / t.toplam).toFixed(1) + '%' })]));
        }
        const parcalar = [];
        if (t.toplam) parcalar.push(t.i + ' / ' + t.toplam);
        if (t.kalan_sn != null) parcalar.push('kalan ' + sureMetni(t.kalan_sn));
        if (t.mola_kalan_sn != null) parcalar.push('mola: ' + t.mola_kalan_sn + ' sn');
        if (t.mod !== 'ptt' && t.mod !== 'takip_listesi' && GT_KESIN.indexOf(t.mod) < 0) parcalar.push(t.alinan + ' dosya alındı', t.yeni_dosya + ' yeni');
        if (t.mod === 'kesinlesme') parcalar.push(t.kes_dosya + ' dosya okundu', t.kes_kesinlesen + ' borçlu UYAP\'ta kesinleşmiş');
        if (t.mod === 'kesinlestirme_talebi') {
            parcalar.push(t.talep_gonderilen + ' talep gönderildi');
            if (t.talep_kesinlesmis) parcalar.push(t.talep_kesinlesmis + ' borçlu zaten kesinleşmiş');
            if (t.talep_atlanan + t.talep_hatasi) parcalar.push((t.talep_atlanan + t.talep_hatasi) + ' gönderilemedi');
        }
        if (t.mod === 'mernis') {
            parcalar.push(t.mer_sorulan + ' Mernis sorgusu' + (t.mer_onbellek ? ', ' + t.mer_onbellek + ' önceki sorgudan' : ''));
            parcalar.push(t.mer_uygun + ' uygun (21/2)', t.mer_farkli + ' farklı adres', t.mer_elle + ' elle incelenmeli');
            if (t.mer_hata) parcalar.push(t.mer_hata + ' sorgulanamadı');
        }
        if (t.mod === 'yeniden_teblig') {
            parcalar.push(t.yt_gonderilen + ' talep gönderildi (≈ ' + sayi(t.yt_tutar) + ' TL)');
            if (t.yt_atlanan + t.yt_hatasi) parcalar.push((t.yt_atlanan + t.yt_hatasi) + ' gönderilmedi');
        }
        if (t.takip_okunan || t.takip_okunamayan) parcalar.push(t.takip_okunan + ' dosyanın takip türü okundu' + (t.takip_okunamayan ? ' (' + t.takip_okunamayan + ' alınamadı)' : ''));
        if (t.taraf_cekilen) parcalar.push(t.taraf_cekilen + ' dosyanın tarafları / borçlu kimlikleri alındı');
        if (t.kapandi) parcalar.push(t.kapandi + ' kapandı');
        if (t.mod === 'tebligat' || t.mod === 'ptt') {
            if (t.pdf_indirilen) parcalar.push(t.pdf_indirilen + ' tebligat belgesi');
            if (t.ptt_sorulan) parcalar.push(t.ptt_sorulan + ' PTT sorgusu');
            parcalar.push(t.teb_yazilan + ' tebligat yazıldı (' + t.teb_yeni + ' yeni, ' + t.teb_degisen + ' durumu değişti)');
            if (t.ptt_kapandi) parcalar.push('PTT yanıt vermiyor, bu tur bırakıldı');
        }
        kutu.appendChild(e('div', { sinif: 'i-gt-not', metin: parcalar.join(' · ') }));
        if (t.captcha) gtCaptchaCiz(kutu, t);
        if (t.uyarilar && t.uyarilar.length) {
            kutu.appendChild(e('ul', { sinif: 'i-gt-uyarilar' }, t.uyarilar.slice(-5).map(function(u) { return e('li', { metin: u }); })));
        }
        const dur = e('button', { sinif: 'i-d tehlike', type: 'button', metin: 'Durdur', stil: 'margin-top:8px',
            onclick: async function() {
                dur.disabled = true;
                try { d.gt = await gtIstek('/' + t.id + '/iptal', {}); } catch (x) { bildir(x.message); }
                gtYokla();
            } });
        kutu.appendChild(dur);
    }
    function gtSonucCiz(kutu, t) {
        const tur = 'Son güncelleme: ' + gtTurAdi(t).toLocaleLowerCase('tr-TR');
        const ad = tur + ({ bitti: ' tamamlandı', iptal: ' durduruldu', hata: ' hatayla bitti' }[t.durum] || '');
        kutu.appendChild(e('div', { stil: 'font-weight:600', metin: ad + (t.kapsam ? ' (' + t.kapsam + ')' : '') }));
        if (t.mesaj) kutu.appendChild(e('div', { sinif: 'i-gt-not', metin: t.mesaj }));
        if (t.rapor && t.rapor.length) kutu.appendChild(e('ul', { sinif: 'i-gt-rapor' }, t.rapor.map(function(s) { return e('li', { metin: s }); })));
        if (t.uyarilar && t.uyarilar.length) {
            kutu.appendChild(e('details', {}, [e('summary', { metin: t.uyari_sayisi + ' uyarı / not' + (t.uyari_sayisi > t.uyarilar.length ? ' (son ' + t.uyarilar.length + ')' : '') }),
                e('ul', { sinif: 'i-gt-uyarilar' }, t.uyarilar.map(function(u) { return e('li', { metin: u }); }))]));
        }
    }
    // Toplu taramanın dosyaları: seçim varsa seçili dosyalar, yoksa açık sekmenin süzgecindeki bütün dosyalar (bütün sayfalar).
    async function gtKapsamIdleri() {
        const l = seciliListe();
        const ek = l ? ' ("' + l.ad + '" listesi içinde)' : '';
        if (d.secim.size) return { ad: 'Satırlarda seçili dosyalar', idler: Array.from(d.secim) };
        if (d.sekme === 'surec') return { ad: 'Süreçler süzgecindeki dosyalar' + ek, idler: await istek('dosya_kimlikleri?' + listeSorgusu()) };
        if (d.sekme === 'tebligat') return { ad: 'Tebligatlar süzgecindeki dosyalar' + ek, idler: await istek('tebligat_dosya_kimlikleri?' + tebSorgusu(true)) };
        throw new Error('Toplu tarama bu sekmede yapılmaz; Süreçler ya da Tebligatlar sekmesine geçin.');
    }
    function gtFormCiz(kutu, t) {
        const f = d.gtForm;
        kutu.appendChild(e('div', { sinif: 'i-gt-not', stil: 'margin-top:6px',
            metin: 'İstekler ofis bilgisayarından sırayla ve yavaş gider; sayfayı kapatsanız da sürer.' }));
        // Kapsam (kullanıcı kuralı 23.11.2026): seçim varsa yalnız seçili dosyalar, yoksa bu sekmenin süzgecindeki
        // dosyalar. Süzgeç dışındaki hiçbir dosyaya istek gitmez; daire / tarih kapsamı yok (süzgeçle daraltılır).
        const kapsamMetni = e('b', { sinif: 'i-gt-not', metin: '…' });
        kutu.appendChild(e('div', { sinif: 'i-gt-form' }, [e('span', { sinif: 'i-gt-not', metin: 'Kapsam:' }), kapsamMetni]));
        gtKapsamIdleri().then(function(k) {
            kapsamMetni.textContent = k.ad + ' (' + k.idler.length + ' dosya)' + (k.idler.length ? '' : ' — işlem yapılacak dosya yok');
        }).catch(function(x) { kapsamMetni.textContent = x.message; });
        const baslat = function(mod, dugme) {
            return async function() {
                dugme.disabled = true;
                const govde = { mod: mod };
                if (mod === 'tebligat') govde.ptt = !!f.ptt;
                try {
                    // "Dosya listesini güncelle" UYAP'tan tek bir açık dosya listesi alır (dosya dosya istek yok); kapsamsız.
                    if (mod !== 'liste') {
                        const k = await gtKapsamIdleri();
                        if (!k.idler.length) { bildir('Kapsamda dosya yok; istek gönderilmedi.'); dugme.disabled = false; return; }
                        govde.dosyalar = k.idler;
                    }
                    d.gt = await gtIstek('', govde);
                    gtYokla();
                } catch (x) { bildir(x.message); dugme.disabled = false; }
            };
        };
        const liste = e('button', { sinif: 'i-d ana', type: 'button', metin: 'Açık dosya listesini UYAP\'tan al',
            title: 'UYAP\'taki açık dosyaların listesi (tek liste): yeni dosyalar eklenir, kapanan dosyalar işaretlenir. Süzgeçten bağımsızdır; taraf ve tebligat istenmez.' });
        liste.addEventListener('click', baslat('liste', liste));
        const taraf = e('button', { sinif: 'i-d', type: 'button', metin: 'Taraf bilgilerini güncelle',
            title: 'Yalnız tarafı ya da borçlu TCKN / vergi no\'su eksik dosyalar sorgulanır' });
        taraf.addEventListener('click', baslat('taraf', taraf));
        const takip = e('button', { sinif: 'i-d', type: 'button', metin: 'Takip türü bilgisi',
            title: 'Takip türü, yolu ve şekli henüz okunmamış dosyalar sorgulanır (dosya başına 1 istek). Kesinleşme süresi buna göre seçilir.' });
        takip.addEventListener('click', baslat('takip', takip));
        kutu.appendChild(e('div', { sinif: 'i-gt-form' }, [liste, taraf, takip]));
        const tebligat = e('button', { sinif: 'i-d', type: 'button', metin: 'Tebligat bilgilerini güncelle',
            title: 'Her dosyanın tebligat evrakları: barkod, tür, mazbata ve (seçiliyse) PTT durumu. Barkodu bilinen evrakın belgesi yeniden indirilmez.' });
        tebligat.addEventListener('click', baslat('tebligat', tebligat));
        const pttK = e('input', { type: 'checkbox', 'aria-label': 'PTT durumunu da sorgula' });
        pttK.checked = !!f.ptt;
        pttK.addEventListener('change', function() { f.ptt = pttK.checked; });
        const yalnizPtt = e('button', { sinif: 'i-d', type: 'button', metin: 'Yalnız PTT',
            title: 'UYAP\'a gitmeden, kayıtlı ve sonucu kesinleşmemiş barkodların PTT durumunu sorgular' });
        yalnizPtt.addEventListener('click', baslat('ptt', yalnizPtt));
        kutu.appendChild(e('div', { sinif: 'i-gt-form' }, [tebligat,
            e('label', { sinif: 'i-gt-not' }, [pttK, ' PTT durumunu da sorgula']), yalnizPtt]));
        kutu.appendChild(e('div', { sinif: 'i-gt-not', stil: 'margin-top:6px',
            metin: 'Taraf bilgileri dosya başına 1–2, takip türü 1, tebligat bilgileri 2–4 istek atar (~3 sn arayla); çok dosyada saatler sürebilir; önce süzgeçle ya da seçimle daraltın. Takip türü bir kez okunur; dosya listesi güncellemesi yeni dosyaların takip türünü kendisi alır. PTT sorgusu barkod başına ~10 sn sürer.' }));
        if (mesaiMi()) {
            kutu.appendChild(e('div', { sinif: 'i-gt-uyari', role: 'note',
                metin: 'Mesai saatindesiniz: tarama UYAP\'ı yavaşlatabilir ve büronun sorgularıyla çakışabilir. Akşam 18:00\'den sonra çalıştırmanız önerilir.' }));
        }
    }
    // Güvenlik doğrulaması: UYAP'ın kendi Turnstile site anahtarıyla, kullanıcı çözer (Gerekçeli Kararlar paneliyle aynı).
    function tsYukle() {
        if (window.turnstile) return Promise.resolve(window.turnstile);
        if (tsYukleniyor) return tsYukleniyor;
        tsYukleniyor = new Promise(function(res, rej) {
            const sc = document.createElement('script');
            sc.src = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit';
            sc.async = true;
            sc.onload = function() { res(window.turnstile); };
            sc.onerror = function() { tsYukleniyor = null; rej(new Error('doğrulama betiği yüklenemedi')); };
            document.head.appendChild(sc);
        });
        return tsYukleniyor;
    }
    // onEk: '' (toplu tarama) ya da '/dosya' (tek dosya işi); sonra: çözümden sonra yoklayan işlev.
    function gtCaptchaCiz(kutu, t, onEk, sonra) {
        const c = t.captcha;
        kutu.setAttribute('data-captcha', String(c.n));
        const yer = e('div', { stil: 'margin-top:6px;min-height:65px' });
        const cevap = async function(jeton) {
            try {
                const yeni = await gtIstek((onEk || '') + '/' + t.id + '/captcha', { token: jeton });
                if (!onEk) d.gt = yeni;
            } catch (x) { bildir(x.message); }
            kutu.removeAttribute('data-captcha');
            (sonra || gtYokla)();
        };
        kutu.appendChild(e('div', { sinif: 'i-gt-uyari', metin: c.mesaj || 'UYAP güvenlik doğrulaması istiyor.' }));
        kutu.appendChild(yer);
        kutu.appendChild(e('button', { sinif: 'i-d kucuk', type: 'button', metin: 'Tekrar dene', onclick: function() { cevap(null); } }));
        tsYukle().then(function(ts) {
            ts.render(yer, { sitekey: c.site_key, callback: cevap,
                'error-callback': function(kod) {
                    yer.appendChild(e('div', { sinif: 'i-gt-uyari', metin: 'Doğrulama açılamadı (kod: ' + kod + '). Biraz bekleyip "Tekrar dene"ye basın.' }));
                    return true;
                } });
        }).catch(function(x) { yer.appendChild(e('div', { sinif: 'i-gt-uyari', metin: 'Doğrulama gösterilemedi (' + x.message + ').' })); });
    }

    // ── Dosya paneli ──
    async function dosyaAc(id) {
        try {
            const v = await istek('dosya?id=' + id);
            const g = await istek('gecmis?dosya=' + id);
            d.secili = { id: id, veri: v, gecmis: g };
            aktifCiz();
        } catch (x) { bildir(x.message); }
    }
    async function seciliyiTazele() {
        if (!d.secili) return;
        const id = d.secili.id;
        await dosyaAc(id);
        if (d.sekme !== 'surec') { yukle(); return; }
        try {
            const sonuc = await Promise.all([istek('ozet'), istek('borclular?' + listeSorgusu())]);
            d.ozet = sonuc[0]; d.liste = sonuc[1];
            durumYaz(); surecCiz();
        } catch (x) {}
    }

    async function uyaptaAc(kart) {
        if (!window.__uyapTazeKart || !window.__uyapKartiAcDogrudan) { bildir('Dosya kartı bu sayfadan açılamıyor.'); return; }
        // Sayfa kapanmaz: kart üstünde açılır, kart küçültülünce / kapatılınca kullanıcı buraya döner.
        if (!document.getElementById('searchId')) {
            history.pushState({}, '', '/');
            window.dispatchEvent(new PopStateEvent('popstate', { state: {} }));
            const bas = Date.now();
            while (!document.getElementById('searchId') && Date.now() - bas < 8000) await new Promise(function(r) { setTimeout(r, 150); });
            if (!document.getElementById('searchId')) { bildir('UYAP ana sayfası açılamadı.'); return; }
        }
        bildir('Dosya UYAP\'ta aranıyor…');
        const t = await window.__uyapTazeKart(kart).catch(function() { return null; });
        if (!t || !t.dosyaId) { bildir('Dosya UYAP\'ta bulunamadı: ' + kart.birimAdi + ' ' + kart.dosyaNo); return; }
        if (window.__uyapDosyaAnahtari) icradanAcilan.add(window.__uyapDosyaAnahtari(t));
        if (!window.__uyapKartiAcDogrudan(t)) bildir('Dosya kartı açılamadı (UYAP sayfası değişmiş olabilir).');
    }

    // ── Dosya panelinden tek dosya güncellemesi: künye (dosya no, durum), taraflar, tebligatlar (tümü ya da seçilenler).
    // Toplu taramayı beklemez; istek sayısı az, sonuç birkaç saniyede gelir. (icra_guncelle.rs Mod::Dosya)
    let dosyaIsZaman = null;
    function dosyaIsSuruyor(id) { const x = d.dosyaIs; return !!(x && x.dosya === id && x.t && GT_SON.indexOf(x.t.durum) < 0); }
    async function dosyaGuncelle(id, govde) {
        try {
            const t = await gtIstek('/dosya', Object.assign({ dosya: id }, govde));
            d.dosyaIs = { dosya: id, t: t };
            dosyaIsCizYerinde();
            dosyaIsYokla();
        } catch (x) { bildir(x.message); }
    }
    async function dosyaIsYokla() {
        clearTimeout(dosyaIsZaman);
        const x = d.dosyaIs;
        if (!x || !x.t) return;
        let t;
        try { t = await gtIstek('/dosya/' + x.t.id); }
        catch (er) { if (d.dosyaIs === x) dosyaIsZaman = setTimeout(dosyaIsYokla, 5000); return; }
        if (d.dosyaIs !== x) return;
        x.t = t;
        if (GT_SON.indexOf(t.durum) < 0) { dosyaIsZaman = setTimeout(dosyaIsYokla, 1500); dosyaIsCizYerinde(); return; }
        bildir(t.mesaj || 'Bitti.');
        if (d.secili && d.secili.id === x.dosya) { if (d.tebSecim) d.tebSecim.liste = []; seciliyiTazele(); }
        else dosyaIsCizYerinde();
    }
    function dosyaIsCizYerinde() {
        const eski = kok && kok.querySelector('.i-dosya-is');
        if (!eski || !d.secili) return;
        const x = d.dosyaIs;
        // Doğrulama kutusu açıkken yeniden çizilmez (çözülen kutu kaybolmasın).
        if (x && x.t && x.t.captcha && eski.getAttribute('data-captcha') === String(x.t.captcha.n)) return;
        eski.replaceWith(dosyaIsKutusu(d.secili.id));
        const dugme = kok.querySelector('.i-dosya-guncelle');
        if (dugme) dugme.disabled = dosyaIsSuruyor(d.secili.id);
        const teb = kok.querySelector('.i-dosya-teb');
        if (teb) teb.disabled = dosyaIsSuruyor(d.secili.id);
    }
    function dosyaIsKutusu(id) {
        const x = d.dosyaIs;
        const kutu = e('div', { sinif: 'i-dosya-is' });
        if (!x || x.dosya !== id || !x.t) return kutu;
        const t = x.t;
        kutu.classList.add('i-gt');
        kutu.setAttribute('role', 'status');
        if (GT_SON.indexOf(t.durum) < 0) {
            kutu.appendChild(e('div', { stil: 'font-weight:600', metin: t.dosya_kesinlesme ? 'Veritabanı güncelleniyor…' : 'Taraf ve tebligatlar alınıyor…' }));
            kutu.appendChild(e('div', { sinif: 'i-gt-not', metin: t.mesaj || '' }));
            if (t.captcha) gtCaptchaCiz(kutu, t, '/dosya', dosyaIsYokla);
            kutu.appendChild(e('button', { sinif: 'i-d kucuk tehlike', type: 'button', metin: 'Durdur', stil: 'margin-top:6px',
                onclick: async function() { try { await gtIstek('/dosya/' + t.id + '/iptal', {}); } catch (er) { bildir(er.message); } dosyaIsYokla(); } }));
            return kutu;
        }
        kutu.appendChild(e('div', { stil: 'display:flex;gap:8px;align-items:flex-start' }, [
            e('div', { stil: 'flex:1;font-weight:600', metin: t.mesaj || '' }),
            e('button', { sinif: 'i-d kucuk', type: 'button', metin: 'Kapat', onclick: function() { d.dosyaIs = null; dosyaIsCizYerinde(); } })]));
        if (t.rapor && t.rapor.length) kutu.appendChild(e('ul', { sinif: 'i-gt-rapor' }, t.rapor.map(function(r) { return e('li', { metin: r }); })));
        if (t.uyarilar && t.uyarilar.length) {
            kutu.appendChild(e('details', {}, [e('summary', { metin: t.uyari_sayisi + ' uyarı / not' }),
                e('ul', { sinif: 'i-gt-uyarilar' }, t.uyarilar.map(function(u) { return e('li', { metin: u }); }))]));
        }
        if (t.durum === 'bitti' && (t.talep_adaylari || []).length) kutu.appendChild(panelTalepKutusu(x));
        return kutu;
    }
    // "Veritabanını güncelle" sonunda kesinleştirilebilir borçlular: talep yalnız onayla gider (Mod::Talep, e-imzalı;
    // göndermeden hemen önce UYAP'ta kesinleşmiş mi diye yeniden bakılır). Toplu bir UYAP işi sürerken gönderilmez
    // (iki iş aynı anda e-imza kartını kullanamaz).
    function panelTalepKutusu(x) {
        const adaylar = x.t.talep_adaylari;
        if (!x.talepSecim) x.talepSecim = adaylar.map(function(a) { return a.bag; });
        const kutu = e('div', { sinif: 'i-panel-talep', role: 'region', 'aria-label': 'Kesinleştirme talebi', stil: 'margin-top:10px;border-top:1px solid var(--i-kenar);padding-top:10px' });
        kutu.appendChild(e('div', { stil: 'font-weight:600;margin-bottom:4px', metin: 'Kesinleştirme talebi' }));
        if (x.talepDurum === 'gonderildi') {
            kutu.appendChild(e('div', { sinif: 'i-gt-not', metin: 'Talep işi başladı; ilerleme ve sonuç üstteki UYAP işi panelinde görünür.' }));
            return kutu;
        }
        kutu.appendChild(e('div', { sinif: 'i-gt-not', stil: 'color:var(--i-yazi)', metin: adaylar.length + ' borçlunun kesinleşme süresi dolmuş, mazbatası dönmüş ve UYAP\'ta kesinleşmemiş:' }));
        adaylar.forEach(function(a) {
            const k = e('input', { type: 'checkbox', checked: x.talepSecim.indexOf(a.bag) >= 0, stil: 'margin:2px 8px 0 0', 'aria-label': 'Talep gönderilecek: ' + a.borclu });
            k.addEventListener('change', function() {
                const i = x.talepSecim.indexOf(a.bag);
                if (k.checked && i < 0) x.talepSecim.push(a.bag);
                if (!k.checked && i >= 0) x.talepSecim.splice(i, 1);
                dosyaIsCizYerinde();
            });
            kutu.appendChild(e('label', { stil: 'display:flex;align-items:flex-start;cursor:pointer;margin:3px 0' },
                [k, e('span', { metin: a.borclu + (a.grup === 'talep_sonucsuz' ? ' (önceki talepten sonra kesinleşme görülmedi)' : '') })]));
        });
        kutu.appendChild(e('div', { sinif: 'i-gt-not', metin: 'Her talep UYAP\'ta hazırlanır, takılı e-imza kartıyla imzalanır ve gönderilir; gönderilen talep geri alınamaz. Göndermeden hemen önce borçlunun UYAP\'ta kesinleşmiş olup olmadığına yeniden bakılır.' }));
        const mesgul = gtSuruyor();
        if (mesgul) kutu.appendChild(e('div', { sinif: 'i-gt-uyari', role: 'note', metin: 'Toplu bir UYAP işi sürüyor; o bitince buradan gönderebilirsiniz.' }));
        const n = x.talepSecim.length;
        kutu.appendChild(e('div', { sinif: 'i-eylem' }, [
            e('button', { sinif: 'i-d kucuk ana', type: 'button', metin: 'E-imzala ve gönder (' + n + ' borçlu)', disabled: mesgul || !n || x.talepDurum === 'gidiyor',
                onclick: function() { panelTalepGonder(x); } }),
            e('button', { sinif: 'i-d kucuk', type: 'button', metin: 'Vazgeç', onclick: function() { x.t.talep_adaylari = []; dosyaIsCizYerinde(); } })
        ]));
        return kutu;
    }
    async function panelTalepGonder(x) {
        const mesgulUyari = function() { bildir('Toplu bir UYAP işi sürüyor; o bitince buradan gönderebilirsiniz.'); x.talepDurum = null; dosyaIsCizYerinde(); };
        x.talepDurum = 'gidiyor';
        dosyaIsCizYerinde();
        try {
            // Başlatma ucu süren bir toplu iş varsa onu döndürür: önce sorulur, talep o işle karışmaz.
            const aktif = await gtIstek('');
            if (aktif && GT_SON.indexOf(aktif.durum) < 0) { d.gt = aktif; mesgulUyari(); return; }
            const t = await gtIstek('', { mod: 'kesinlestirme_talebi', dosyalar: [x.dosya], borclular: x.talepSecim.slice() });
            if (!t || t.mod !== 'kesinlestirme_talebi') { d.gt = t; mesgulUyari(); return; }
            x.talepDurum = 'gonderildi';
            d.gt = t;
            d.gtAcik = true;
            gtYokla();
        } catch (er) { x.talepDurum = null; bildir(er.message); }
        dosyaIsCizYerinde();
    }

    function panelCiz() {
        const s = d.secili, v = s.veri, k = v.kunye, t = v.takip;
        const panel = e('div', { sinif: 'i-panel' });
        const rozetler = [k.acik ? 'UYAP: Açık' : 'UYAP: ' + (k.durum || 'Kapalı'), 'İşlem: ' + k.islem_durumu_etiket];
        if (t.turu_ad) rozetler.push(t.turu_ad + (t.ornek_no ? ' · Örnek ' + t.ornek_no : ''));
        rozetler.push(k.talimat ? 'Talimat dosyası: kesinleşme yok' : t.kesinlesme_yok ? t.kesinlesme_yok
            : t.kesinlesme_suresi_gun ? 'Kesinleşme süresi ' + t.kesinlesme_suresi_gun + ' gün' : 'Kesinleşme süresi tanımlı değil');
        if (!k.talimat && t.itiraz_durdurur === false) rozetler.push('İtiraz takibi durdurmaz');
        // MTS (7155 s. K.): icra dairesine geçen dosyada yeni ödeme emri yoktur, kesinleşme MTS'deki tebliğe göredir.
        if (k.mts_kaynakli) rozetler.push('MTS\'den geldi: kesinleşme MTS ödeme emrine göre');
        if (k.mts_icraya_gecti) rozetler.push('MTS: icra takibine geçti (yeni esas dosyasında izlenir)');
        panel.appendChild(e('div', { sinif: 'p-bas' }, [
            e('div', { stil: 'display:flex;justify-content:space-between;gap:10px;align-items:flex-start' }, [
                e('div', {}, [e('h2', { metin: k.dosya_no }), e('div', { sinif: 'alt', metin: k.birim + (k.tur ? ' · ' + k.tur : '') + (k.acilis_tarihi ? ' · Açılış ' + gunMetni(k.acilis_tarihi) : '') })]),
                e('div', { stil: 'display:flex;gap:6px;flex-wrap:wrap;justify-content:flex-end' }, [
                    e('button', { sinif: 'i-d kucuk ana i-dosya-guncelle', metin: 'Veritabanını güncelle', disabled: dosyaIsSuruyor(k.id),
                        title: 'Bu dosyanın künyesini, taraflarını ve tebligatlarını UYAP\'tan çeker, barkodları PTT\'ye sorar. Kesinleşme süresi dolmuş ve mazbatası dönmüş borçlu varsa kesinleşmeyi UYAP\'tan okur; kesinleştirme talebi için ayrıca onayınızı ister. Toplu taramayı beklemez.',
                        onclick: function() { dosyaGuncelle(k.id, { taraf: true, tebligat: true, kesinlesme: true }); } }),
                    e('button', { sinif: 'i-d kucuk i-dosya-teb', metin: 'Tebligatları güncelle', disabled: dosyaIsSuruyor(k.id),
                        title: 'Yalnız bu dosyanın tebligatları: evrak listesi, yeni tebligat belgeleri ve PTT durumu (künye, taraf ve kesinleşme sorulmaz).',
                        onclick: function() { dosyaGuncelle(k.id, { taraf: false, tebligat: true }); } }),
                    e('button', { sinif: 'i-d kucuk', metin: 'UYAP\'ta aç', title: 'Dosyayı UYAP\'ta bulup kartını açar', onclick: function() { uyaptaAc(k.uyap); } }),
                    e('button', { sinif: 'i-d kucuk', metin: '✕', title: 'Paneli kapat', 'aria-label': 'Paneli kapat', onclick: function() { d.secili = null; aktifCiz(); } })
                ])
            ]),
            e('div', { sinif: 'i-rozet' }, rozetler.map(function(r) { return e('span', { metin: r }); })),
            dosyaIsKutusu(k.id)
        ]));
        panel.appendChild(e('div', { sinif: 'i-sekmeler p-sekmeler' }, [['borclular', 'Borçlular'], ['tebligat', 'Tebligatlar (' + v.tebligatlar.length + ')' + ((v.alinamayan || []).length ? ' · ' + v.alinamayan.length + ' alınamadı' : '')], ['gecmis', 'Geçmiş'], ['belgeler', 'Belgeler'], ['kunye', 'Dosya']].map(function(x) {
                return e('button', { sinif: d.panelSekme === x[0] ? 'on' : '', metin: x[1], onclick: function() { d.panelSekme = x[0]; aktifCiz(); } });
        })));
        const ic = e('div', { sinif: 'p-ic' });
        panel.appendChild(ic);
        if (d.panelSekme === 'borclular') borclularCiz(ic, v);
        else if (d.panelSekme === 'tebligat') tebligatlarCiz(ic, v);
        else if (d.panelSekme === 'gecmis') gecmisCiz(ic, s.gecmis);
        else if (d.panelSekme === 'belgeler') belgelerCiz(ic, v);
        else kunyeCiz(ic, v);
        govde.appendChild(panel);
    }

    // Panelden tek borçlu için toplu iş altyapısıyla (Mod::Mernis / Mod::YenidenTeblig). Süren toplu iş varsa başlatılmaz.
    async function panelUyapIsi(govde) {
        try {
            const aktif = await gtIstek('');
            if (aktif && GT_SON.indexOf(aktif.durum) < 0) { d.gt = aktif; bildir('Toplu bir UYAP işi sürüyor; o bitince yeniden deneyin.'); return false; }
            const t = await gtIstek('', govde);
            if (!t || t.mod !== govde.mod) { d.gt = t; bildir('Toplu bir UYAP işi sürüyor; o bitince yeniden deneyin.'); return false; }
            d.gt = t;
            d.gtAcik = true;
            gtYokla();
            bildir('Başladı; ilerleme üstteki UYAP işi panelinde.');
            return true;
        } catch (x) { bildir(x.message); return false; }
    }
    function borcluMenusu(ev, v, b) {
        const m = b.mernis, yt = b.yeniden_talep;
        const gruplar = [[['Borçlu · ' + b.ad, null]]];
        const mer = [];
        if (b.teblig_kodu === 'iade' || m) {
            mer.push([m ? 'Mernis adresini yeniden sorgula (son: ' + m.zaman + ')' : 'Mernis adresini sorgula ve ilk tebligatla karşılaştır',
                function() { panelUyapIsi({ mod: 'mernis', dosyalar: [v.kunye.id], borclular: [b.bag], yeniden: true }); }]);
        } else {
            mer.push(['Mernis sorgusu: ödeme emri iade dönmemiş', null]);
        }
        gruplar.push(mer);
        if (yt && (yt.sonuc === 'gonderildi' || yt.sonuc === 'zaten')) {
            gruplar.push([['Yeniden tebliğ talebi: ' + (yt.sonuc === 'gonderildi' ? 'gönderildi ' : 'dosyada var ') + (yt.zaman || ''), null]]);
        } else if (m && m.talep_turu) {
            gruplar.push([['Yeniden tebliğ talebi gönder — ' + m.talep_turu + ' (≈ ' + sayi(m.tutar) + ' TL)…',
                function() { d.ytOnay = { dosya: v.kunye.id, bag: b.bag, ad: b.ad, m: m, anladim: false }; d.panelSekme = 'borclular'; aktifCiz(); }]]);
        } else {
            gruplar.push([[m ? 'Yeniden tebliğ: ' + m.etiket : 'Yeniden tebliğ: önce Mernis sorgusu gerekir', null]]);
        }
        sagMenu(ev, gruplar);
    }
    function ytOnayKutusu(v) {
        const o = d.ytOnay;
        if (!o || o.dosya !== v.kunye.id) return null;
        const kutu = e('div', { sinif: 'i-gt i-yt-onay', role: 'region', 'aria-label': 'Yeniden tebliğ talebi' }, [
            e('div', { stil: 'font-weight:600;margin-bottom:6px', metin: 'Yeniden tebliğ talebi — ' + o.ad }),
            e('div', { sinif: 'i-gt-not', stil: 'color:var(--i-yazi)', metin: o.m.talep_turu + ', adres türü Mernis. ' + o.m.aciklama + '.' }),
            e('div', { sinif: 'i-gt-not', metin: 'Mernis adresi: ' + (o.m.adres || '—') }),
            e('div', { sinif: 'i-gt-not', metin: 'İlk tebligat adresi: ' + (o.m.teblig_adresi || '—') }),
            e('div', { sinif: 'i-gt-uyari', metin: 'ÜCRETLİDİR: ≈ ' + sayi(o.m.tutar) + ' TL e-Barobirlik kartından düşer; gönderilen talep geri alınamaz. Göndermeden hemen önce dosyada talep evrakı olup olmadığına bakılır.' }),
            e('label', { stil: 'display:flex;gap:6px;align-items:flex-start;cursor:pointer;margin-top:6px' }, [
                e('input', { type: 'checkbox', checked: o.anladim, 'aria-label': 'Ücretli olduğunu anladım', onchange: function(ev) { o.anladim = ev.target.checked; aktifCiz(); } }),
                'Ücretli olduğunu ve geri alınamayacağını anladım.']),
            e('div', { sinif: 'i-eylem' }, [
                e('button', { sinif: 'i-d kucuk ana', type: 'button', disabled: !o.anladim, metin: 'E-imzala ve gönder (≈ ' + sayi(o.m.tutar) + ' TL)',
                    onclick: async function() {
                        const tamam = await panelUyapIsi({ mod: 'yeniden_teblig', dosyalar: [o.dosya], borclular: [o.bag] });
                        if (tamam) { d.ytOnay = null; aktifCiz(); }
                    } }),
                e('button', { sinif: 'i-d kucuk', type: 'button', metin: 'Vazgeç', onclick: function() { d.ytOnay = null; aktifCiz(); } })
            ])
        ]);
        return kutu;
    }

    function borclularCiz(ic, v) {
        const borclular = v.taraflar.filter(function(x) { return x.rol === 'borclu'; });
        const digerleri = v.taraflar.filter(function(x) { return x.rol !== 'borclu'; });
        const ytKutu = ytOnayKutusu(v);
        if (ytKutu) ic.appendChild(ytKutu);
        if (!borclular.length) ic.appendChild(e('div', { sinif: 'i-bilgi', metin: 'Dosyada kayıtlı borçlu yok. Üstteki "Veritabanını güncelle" ile taraflar çekilebilir.' }));
        borclular.forEach(function(b) {
            const adim = SIRA.indexOf(b.asama);
            const sonuc = { iade: 5, itiraz: 6, durduruldu: 6 }[b.asama];
            const dolu = adim >= 0 ? adim + 1 : (sonuc || 0);
            const tarih = e('input', { type: 'date', value: b.itiraz_tarihi || '' });
            const elle = e('select', { title: 'Elle girilen kesinleşme değeri (hesap ezmez)' }, [
                ['', 'Elle değer yok'], ['konkordato', 'Konkordato'], ['vefat', 'Vefat'], ['vasi', 'Vasi'], ['tehiri_icra', 'Tehir-i İcra']
            ].map(function(o) { const op = e('option', { value: o[0], metin: o[1] }); if (b.kesinlesme_durumu === o[0]) op.selected = true; return op; }));
            if (['konkordato', 'vefat', 'vasi', 'tehiri_icra'].indexOf(b.kesinlesme_durumu) < 0) elle.value = '';
            elle.addEventListener('change', async function() {
                try { await istek('kesinlesme_elle', { bag: b.bag, durum: elle.value }); bildir('Kaydedildi.'); seciliyiTazele(); }
                catch (x) { bildir(x.message); }
            });
            // Elle kesinleşme günü (UYAP'ta görülen): gün gelmişse Kesinleşti, gelmemişse o güne Kesinleşme Bekliyor; hesap ezmez.
            const kesGun = e('input', { type: 'date', value: b.kesinlesme_elle_tarihi || '', 'aria-label': 'Kesinleşme günü' });
            const kesKaydet = async function(gun) {
                try {
                    const r = await istek('kesinlesme_tarihi', { bag: b.bag, tarih: gun || null });
                    const m = (r && r.kesinlesme) || '';
                    bildir(gun && m.indexOf('Kesinle') < 0 ? 'Gün kaydedildi ama itiraz / durdurma önce geliyor: ' + (m || '—') : 'Kaydedildi' + (m ? ': ' + m : '.'));
                    seciliyiTazele();
                } catch (x) { bildir(x.message); }
            };
            const kesSatiri = e('div', { sinif: 'i-eylem' }, [
                e('label', { sinif: 'alt', stil: 'display:inline-flex;gap:6px;align-items:center' }, ['Kesinleşme günü', kesGun]),
                e('button', { sinif: 'i-d kucuk ana', type: 'button', metin: 'Kesinleşmeyi kaydet',
                    title: 'UYAP\'ta gördüğünüz kesinleşme günü. Gün geldiyse "Kesinleşti" olur; hesap bunu değiştirmez.',
                    onclick: function() { if (!kesGun.value) { bildir('Önce kesinleşme gününü seçin.'); return; } kesKaydet(kesGun.value); } }),
                b.kesinlesme_elle_tarihi ? e('button', { sinif: 'i-d kucuk', type: 'button', metin: 'Hesaba bırak',
                    title: 'Elle girilen günü siler; kesinleşme yeniden tebligatlardan hesaplanır.', onclick: function() { kesKaydet(null); } }) : null
            ]);
            const itirazDugme = e('button', { sinif: 'i-d kucuk', metin: b.itiraz ? 'İtirazı kaldır' : 'İtiraz edildi',
                onclick: async function() {
                    try { await istek('itiraz', { bag: b.bag, edildi: !b.itiraz, tarih: tarih.value || null }); bildir('Kaydedildi.'); seciliyiTazele(); }
                    catch (x) { bildir(x.message); }
                } });
            const kart = e('div', { sinif: 'i-kart i-borclu', title: 'Sağ tık: Mernis sorgusu / yeniden tebliğ talebi' }, [
                e('div', { sinif: 'b' }, [
                    e('div', {}, [e('div', { sinif: 'ad', metin: b.ad }), e('div', { sinif: 'alt', metin: 'Borçlu' + (b.kimlik ? ' · ' + b.kimlik : '') }),
                                  b.uyapta_gorulmedi ? e('div', { sinif: 'uyari', metin: 'UYAP\'ın taraf listesinde artık görünmüyor' }) : null]),
                    chip(b.asama, null)
                ]),
                e('div', { sinif: 'i-ilerleme', title: 'Takip süreci' }, [0, 1, 2, 3, 4, 5, 6].map(function(i) { return e('i', { sinif: i < dolu ? 'd' : '' }); })),
                e('div', { sinif: 'sat' }, [e('span', { metin: 'Takip aşaması' }), e('b', { metin: b.asama_metni || '—' })]),
                e('div', { sinif: 'sat' }, [e('span', { metin: 'Tebliğ' }), e('b', { metin: b.tebligat || '—' })]),
                e('div', { sinif: 'sat' }, [e('span', { metin: 'Kesinleşme' }), e('b', { metin: v.kunye.talimat ? 'Talimat dosyasında kesinleşme yok' : (b.kesinlesme || '—') })]),
                b.mernis ? e('div', { sinif: 'sat', title: b.mernis.aciklama }, [e('span', { metin: 'Mernis' }), e('b', { metin: b.mernis.etiket })]) : null,
                b.mernis ? e('div', { sinif: 'alt', metin: 'Mernis adresi: ' + (b.mernis.adres || '—') }) : null,
                b.mernis ? e('div', { sinif: 'alt', metin: 'İlk tebligat adresi: ' + (b.mernis.teblig_adresi || '—') }) : null,
                b.yeniden_talep ? e('div', { sinif: 'sat', title: b.yeniden_talep.mesaj || '' }, [e('span', { metin: 'Yeniden tebliğ' }),
                    e('b', { metin: ({ gonderildi: 'Talep gönderildi', zaten: 'Dosyada talep var', hata: 'Gönderilemedi' }[b.yeniden_talep.sonuc] || b.yeniden_talep.sonuc) + (b.yeniden_talep.zaman ? ' · ' + b.yeniden_talep.zaman : '') })]) : null,
                // Talimat dosyasında itiraz / kesinleşme işaretleri anlamsız: yalnız elle değer (vefat vb.) kalır.
                v.kunye.talimat ? null : kesSatiri,
                e('div', { sinif: 'i-eylem' }, v.kunye.talimat ? [elle] : [b.itiraz ? null : e('label', { sinif: 'alt', stil: 'display:inline-flex;gap:6px;align-items:center' }, ['İtiraz günü', tarih]), itirazDugme, elle])
            ]);
            if (!v.kunye.talimat) kart.addEventListener('contextmenu', function(ev) { borcluMenusu(ev, v, b); });
            ic.appendChild(kart);
        });
        if (digerleri.length) {
            ic.appendChild(e('div', { sinif: 'bolum', metin: 'Diğer taraflar' }));
            digerleri.forEach(function(x) {
                ic.appendChild(e('div', { sinif: 'i-kart' }, [e('div', { sinif: 'ad', metin: x.ad }),
                    e('div', { sinif: 'alt', metin: x.rol_etiket + (x.vekiller ? ' · Vekil: ' + x.vekiller : '') })]));
            });
        }
    }

    function tebligatlarCiz(ic, v) {
        const id = v.kunye.id;
        if (!d.tebSecim || d.tebSecim.dosya !== id) d.tebSecim = { dosya: id, liste: [] };
        const sec = d.tebSecim;
        const calisiyor = dosyaIsSuruyor(id);
        const seciliDugme = e('button', { sinif: 'i-d kucuk ana', type: 'button', disabled: calisiyor || !sec.liste.length,
            title: 'İşaretlenen tebligatların belgesi ve PTT durumu UYAP / PTT\'den yeniden alınır',
            onclick: function() { dosyaGuncelle(id, { taraf: false, tebligat: true, evraklar: sec.liste.slice() }); } });
        const seciliYaz = function() { seciliDugme.textContent = 'Seçilenleri sorgula (' + sec.liste.length + ')'; seciliDugme.disabled = calisiyor || !sec.liste.length; };
        seciliYaz();
        ic.appendChild(e('div', { sinif: 'i-eylem', stil: 'margin-bottom:8px' }, [seciliDugme,
            e('button', { sinif: 'i-d kucuk', type: 'button', metin: 'Tümünü sorgula', disabled: calisiyor,
                title: 'Dosyadaki bütün tebligat zarfları (UYAP\'a yeni gelenler dahil). Barkodu bilinen belge yeniden indirilmez.',
                onclick: function() { dosyaGuncelle(id, { taraf: false, tebligat: true }); } })]));
        const secimKutusu = function(no, ad) {
            const k = e('input', { type: 'checkbox', 'aria-label': 'Sorgulanacaklara ekle: ' + ad, checked: sec.liste.indexOf(no) >= 0, stil: 'margin:2px 8px 0 0' });
            k.addEventListener('change', function() {
                const i = sec.liste.indexOf(no);
                if (k.checked && i < 0) sec.liste.push(no);
                if (!k.checked && i >= 0) sec.liste.splice(i, 1);
                seciliYaz();
            });
            return k;
        };
        // Belgelerde olup kaydedilemeyen zarflar: nedeniyle; işaretlenip yeniden sorgulanabilir.
        const alinamayan = v.alinamayan || [];
        if (alinamayan.length) {
            ic.appendChild(e('div', { sinif: 'bolum', metin: 'Belgelerde olup alınamayan zarflar (' + alinamayan.length + ')' }));
            alinamayan.forEach(function(z) {
                ic.appendChild(e('div', { sinif: 'i-kart' }, [
                    e('div', { sinif: 'b' }, [e('label', { stil: 'display:flex;align-items:flex-start;cursor:pointer' },
                        [secimKutusu(z.evrak_no, z.tur), e('div', { sinif: 'ad', metin: z.tur })]), e('span', { sinif: 'alt', metin: z.evrak_tarihi || '' })]),
                    e('div', { sinif: 'alt', metin: z.muhatap || z.aciklama }),
                    e('div', { sinif: 'uyari', metin: z.neden ? 'Alınamadı: ' + z.neden : 'Henüz sorgulanmadı' })
                ]));
            });
            if (v.tebligatlar.length) ic.appendChild(e('div', { sinif: 'bolum', metin: 'Kayıtlı tebligatlar' }));
        }
        if (!v.tebligatlar.length) {
            if (!alinamayan.length) ic.appendChild(e('div', { sinif: 'i-bilgi', metin: 'Bu dosyada kayıtlı tebligat yok. "Tümünü sorgula" ile UYAP\'taki tebligatlar alınabilir.' }));
            return;
        }
        const mazbatalar = v.mazbatalar || [];
        const fiziki = v.tebligatlar.filter(function(t) { return !t.elektronik; });
        v.tebligatlar.forEach(function(t) {
            const hareket = e('div');
            const isaret = secimKutusu(t.evrak_no, (t.tur || 'Tebligat') + ' ' + (t.evrak_tarihi || ''));
            const kart = e('div', { sinif: 'i-kart' }, [
                e('div', { sinif: 'b' }, [e('label', { stil: 'display:flex;align-items:flex-start;cursor:pointer' }, [isaret, e('div', { sinif: 'ad', metin: t.tur || 'Tebligat' })]),
                    e('span', { sinif: 'alt', metin: t.evrak_tarihi })]),
                e('div', { sinif: 'alt', metin: t.muhatap || t.aciklama }),
                e('div', { sinif: 'sat' }, [e('span', { metin: t.elektronik ? 'E-tebligat' : 'Barkod' }), e('b', { metin: t.barkod || 'henüz yok' })]),
                e('div', { sinif: 'sat' }, [e('span', { metin: 'Durum' }), e('b', { metin: (t.ptt_durumu || '—') + (t.son_islem_tarihi ? ' · ' + t.son_islem_tarihi : '') })]),
                t.teslim_sekli ? e('div', { sinif: 'sat' }, [e('span', { metin: 'Teslim / iade' }), e('b', { metin: t.teslim_sekli })]) : null,
                e('div', { sinif: 'sat' }, [e('span', { metin: 'Mazbata' }), e('b', { metin: t.mazbata_var ? (t.mazbata || 'Dosyaya döndü') : t.elektronik ? 'Gerekmez (e-tebligat)' : 'Dönmedi' })]),
                t.borclu ? e('div', { sinif: 'sat' }, [e('span', { metin: 'Borçlu' }), e('b', { metin: t.borclu })]) : null,
                t.yeniden_teblig_talebi ? e('div', { sinif: 'alt', metin: 'Yeniden tebliğ talebi gönderildi' }) : null,
                t.hareket_sayisi ? e('div', { sinif: 'i-eylem' }, [e('button', { sinif: 'i-d kucuk', metin: 'PTT hareketleri (' + t.hareket_sayisi + ')',
                    onclick: async function() {
                        if (hareket.firstChild) { bos(hareket); return; }
                        try {
                            const h = await istek('ptt?tebligat=' + t.id);
                            h.forEach(function(x) {
                                hareket.appendChild(e('div', { sinif: 'sat' }, [e('span', { metin: (x.zaman ? gunMetni(x.zaman) + x.zaman.slice(10) : '') }),
                                    e('b', { metin: x.aciklama + (x.yer ? ' · ' + x.yer : '') + (x.detay ? ' · ' + x.detay : '') })]));
                            });
                        } catch (x) { bildir(x.message); }
                    } })]) : null,
                hareket
            ]);
            if (!t.elektronik) {
                kart.title = 'Sağ tık: mazbatayı bu tebligata bağla';
                kart.addEventListener('contextmenu', function(ev) { tebligatMazbataMenusu(ev, id, t, mazbatalar); });
            }
            ic.appendChild(kart);
        });
        // Fiziki mazbatalar çoğu zaman fotoğraf olarak taranır (barkodsuz): hangi tebligatın olduğu sağ tıkla elle bağlanır.
        if (mazbatalar.length) {
            ic.appendChild(e('div', { sinif: 'bolum', metin: 'Mazbata evrakları (' + mazbatalar.length + ')' }));
            ic.appendChild(e('div', { sinif: 'alt', stil: 'margin:-2px 0 8px', metin: 'Sağ tıkla (dokunmatikte uzun basarak) mazbatayı bir tebligata bağlayabilirsiniz.' }));
            mazbatalar.forEach(function(m) {
                const bagli = m.elle_tebligat ? v.tebligatlar.find(function(t) { return t.evrak_no === m.elle_tebligat; }) : null;
                const kart = e('div', { sinif: 'i-kart i-mazbata', title: 'Sağ tık: tebligata bağla' }, [
                    e('div', { sinif: 'b' }, [e('div', { sinif: 'ad', metin: m.tur }), e('span', { sinif: 'alt', metin: m.evrak_tarihi || '' })]),
                    m.aciklama ? e('div', { sinif: 'alt', metin: m.aciklama }) : null,
                    e('div', { sinif: 'alt', stil: 'margin-top:4px' }, m.elle_tebligat
                        ? ['Elle bağlı: ', e('b', { metin: bagli ? tebligatEtiketi(bagli) : 'evrak ' + m.elle_tebligat }),
                           m.teblig_tarihi ? ' · mazbatadaki tebliğ tarihi ' + m.teblig_tarihi : '']
                        : ['Elle bağ yok'])
                ]);
                kart.addEventListener('contextmenu', function(ev) { mazbataTebligatMenusu(ev, id, m, fiziki); });
                ic.appendChild(kart);
            });
        }
    }
    // ── Mazbata ↔ tebligat elle bağı: ortak yardımcılar (UYAP dosya penceresindeki menü, uyap_proxy.rs, aynı kuralı uygular) ──
    function mazbataHucresi(t) {
        if (t.mazbata_var) return e('span', { sinif: 'mz', title: t.mazbata || 'Mazbata dosyaya döndü', metin: 'Döndü' });
        if (t.elektronik) return e('span', { sinif: 'silik', title: 'E-tebligatta fiziki mazbata dönmez; tebliğ UETS kaydından sayılır.', metin: 'Gerekmez (e-tebligat)' });
        return e('span', { sinif: 'silik', metin: 'Dönmedi' });
    }
    // "26/04/2024", "26.04.2024 10:12", "2024-04-26" → karşılaştırılabilir yyyymmdd sayısı; tarih yoksa 0.
    function gunSayisi(s) {
        let m = /^(\d{2})[\/.](\d{2})[\/.](\d{4})/.exec(String(s || '').trim());
        if (m) return +(m[3] + m[2] + m[1]);
        m = /^(\d{4})-(\d{2})-(\d{2})/.exec(String(s || '').trim());
        return m ? +(m[1] + m[2] + m[3]) : 0;
    }
    function isoGun(s) { const n = gunSayisi(s); return n ? String(n).replace(/^(\d{4})(\d{2})(\d{2})$/, '$1-$2-$3') : ''; }
    function teslimEdildi(t) { return String(t.ptt_durumu || '').toLocaleLowerCase('tr-TR').indexOf('teslim edildi') >= 0; }
    // PTT'de tebliğ günü var mı (eski barkodlar PTT'nin sisteminden silinir: "KAYIT YOK").
    function tebligGunuVar(t) { return teslimEdildi(t) && !!gunSayisi(t.son_islem_tarihi); }
    function tebligatEtiketi(t) {
        const ptt = !t.ptt_durumu ? 'PTT sorgulanmadı'
            : (teslimEdildi(t) ? 'Teslim edildi' : t.ptt_durumu) + (t.son_islem_tarihi ? ' ' + String(t.son_islem_tarihi).slice(0, 10) : '');
        return [String(t.evrak_tarihi || '').slice(0, 10), t.tur || 'Tebligat', t.muhatap, t.barkod || 'barkodsuz', ptt,
                t.mazbata_var ? 'mazbata döndü' : 'mazbata dönmedi'].filter(Boolean).join(' · ');
    }
    // Mazbatanın bağlanabileceği PTT tebligatları. Mazbata kendi tebligatından önce dönemez: evrak günü mazbatadan sonra
    // olan elenir. Önerilen: teslim edilmiş, mazbatası dönmemiş (ya da zaten bu mazbataya bağlı), teslim günü mazbatadan
    // sonra olmayan; birden çoksa teslim günü mazbataya en yakın olan. Yoksa mazbatası dönmemiş en yeni tebligat.
    // (2023/22468: 29.04.2024 mazbatası Kasım 2023 tebligatına bağlanmıştı; asıl sahibi 26.04.2024'te teslim edilendi.)
    function mazbataAdaylari(m, fiziki) {
        const mg = gunSayisi(m.evrak_tarihi);
        const uygun = fiziki.filter(function(t) { const tg = gunSayisi(t.evrak_tarihi); return !mg || !tg || tg <= mg; });
        const serbest = function(t) { return !t.mazbata_var || t.evrak_no === m.elle_tebligat; };
        const puan = function(t) {
            const sg = gunSayisi(t.son_islem_tarihi);
            if (teslimEdildi(t) && serbest(t) && sg && (!mg || sg <= mg)) return 3e8 + sg;
            if (serbest(t)) return 1e8 + gunSayisi(t.evrak_tarihi);
            return gunSayisi(t.evrak_tarihi);
        };
        const liste = uygun.slice().sort(function(a, b) { return puan(b) - puan(a); });
        return { liste: liste, onerilen: liste.length && puan(liste[0]) >= 1e8 ? liste[0] : null, elenen: fiziki.length - uygun.length };
    }
    // Tebliğ günü PTT'de olmayan tebligata bağlarken mazbatadaki tebliğ tarihi sorulur (boş bırakılabilir).
    function tebligTarihiSor(t, m, tamam) {
        if (tebligGunuVar(t)) { tamam(null); return; }
        const ortu = e('div', { sinif: 'i-ortu', role: 'dialog', 'aria-modal': 'true', 'aria-label': 'Mazbatadaki tebliğ tarihi' });
        const kapat = function() { ortu.remove(); };
        ortu.addEventListener('mousedown', function(ev) { if (ev.target === ortu) kapat(); });
        const bugun = new Date();
        const ust = isoGun(m && m.evrak_tarihi) || [bugun.getFullYear(), String(bugun.getMonth() + 1).padStart(2, '0'), String(bugun.getDate()).padStart(2, '0')].join('-');
        const tarih = e('input', { type: 'date', 'aria-label': 'Mazbatadaki tebliğ tarihi', min: isoGun(t.evrak_tarihi) || null, max: ust,
                                   value: isoGun(m && m.teblig_tarihi) || null });
        const bagla = function(deger) { kapat(); tamam(deger); };
        ortu.appendChild(e('div', { sinif: 'i-pencere', stil: 'width:min(480px,calc(100vw - 24px))' }, [
            e('div', { stil: 'font-weight:700;font-size:15px', metin: 'Mazbatadaki tebliğ tarihi' }),
            e('div', { sinif: 'alt', stil: 'margin:6px 0 8px', metin: 'Bu tebligatın tebliğ günü PTT\'de yok (eski barkodlar PTT\'nin sisteminden silinir). Mazbatada yazan tebliğ tarihini girerseniz kesinleşme bu günden hesaplanır.' }),
            e('div', { stil: 'font-weight:600', metin: tebligatEtiketi(t) }),
            e('label', { stil: 'display:flex;align-items:center;gap:8px;margin:12px 0' }, ['Tebliğ tarihi', tarih]),
            e('div', { stil: 'display:flex;gap:8px;justify-content:flex-end;flex-wrap:wrap' }, [
                e('button', { sinif: 'i-d', type: 'button', metin: 'Vazgeç', onclick: kapat }),
                e('button', { sinif: 'i-d', type: 'button', metin: 'Tarihsiz bağla', title: 'Bağ kurulur; kesinleşme için tebliğ günü yine bilinmez', onclick: function() { bagla(null); } }),
                e('button', { sinif: 'i-d ana', type: 'button', metin: 'Bağla', onclick: function() {
                    if (!tarih.value) { bildir('Tebliğ tarihini girin ya da "Tarihsiz bağla"yı seçin.'); return; }
                    bagla(tarih.value);
                } })])
        ]));
        kok.appendChild(ortu);
        tarih.focus();
    }
    async function mazbataBagla(dosya, tebligat, mazbata, tebligTarihi) {
        try {
            await istek('mazbata_bagla', { dosya: dosya, tebligat: tebligat, mazbata: mazbata, teblig_tarihi: tebligTarihi || null });
            bildir(mazbata ? 'Mazbata tebligata bağlandı; tebliğ ve kesinleşme yeniden hesaplandı.' : 'Elle bağ kaldırıldı; otomatik eşleşmeye dönüldü.');
            seciliyiTazele();
        } catch (x) { bildir(x.message); }
    }
    function tarihleBagla(dosya, t, m) {
        tebligTarihiSor(t, m, function(gun) { mazbataBagla(dosya, t.evrak_no, m.evrak_no, gun); });
    }
    function sagMenu(ev, gruplar) {
        if (!window.__uyapSagMenuAc) return;
        ev.preventDefault();
        ev.stopPropagation();
        window.__uyapSagMenuAc(ev.clientX, ev.clientY, gruplar);
    }
    function tebligatMazbataMenusu(ev, dosya, t, mazbatalar) {
        // Mazbata tebligatından önce dönemez: evrak günü tebligattan önce olan mazbata listelenmez.
        const tg = gunSayisi(t.evrak_tarihi);
        const uygun = mazbatalar.filter(function(m) { const mg = gunSayisi(m.evrak_tarihi); return m.elle_tebligat !== t.evrak_no && (!tg || !mg || mg >= tg); });
        const secenek = uygun.map(function(m) {
            return [m.tur + (m.evrak_tarihi ? ' · ' + m.evrak_tarihi : '') + (m.elle_tebligat ? ' (başka tebligattan alınır)' : ''),
                    function() { tarihleBagla(dosya, t, m); }];
        });
        const gruplar = [[['Mazbata bağla · ' + tebligatEtiketi(t), null]].concat(secenek.length ? secenek
            : [[mazbatalar.length ? 'Bu tebligattan sonra dönmüş, bağlanabilecek mazbata yok' : 'Dosyada bağlanabilecek mazbata evrakı yok', null]])];
        if (t.mazbata_elle) {
            const bagli = mazbatalar.find(function(m) { return m.elle_tebligat === t.evrak_no; });
            const g = [['Elle bağı kaldır', function() { mazbataBagla(dosya, t.evrak_no, null); }]];
            if (bagli && !tebligGunuVar(t)) g.unshift([(bagli.teblig_tarihi ? 'Tebliğ tarihini değiştir (' + bagli.teblig_tarihi + ')' : 'Mazbatadaki tebliğ tarihini gir'),
                function() { tarihleBagla(dosya, t, bagli); }]);
            gruplar.push(g);
        }
        sagMenu(ev, gruplar);
    }
    function mazbataTebligatMenusu(ev, dosya, m, fiziki) {
        const a = mazbataAdaylari(m, fiziki);
        const secenek = a.liste.filter(function(t) { return t.evrak_no !== m.elle_tebligat; }).map(function(t) {
            return [(t === a.onerilen ? '★ Önerilen: ' : '') + tebligatEtiketi(t), function() { tarihleBagla(dosya, t, m); }];
        });
        const gruplar = [[['Tebligata bağla · ' + m.tur + (m.evrak_tarihi ? ' ' + m.evrak_tarihi : ''), null]].concat(secenek.length ? secenek
            : [[a.elenen ? 'Bu mazbatadan önce giden başka PTT tebligatı yok' : 'Dosyada PTT ile giden başka tebligat yok', null]])];
        if (m.elle_tebligat) {
            const bagli = fiziki.find(function(t) { return t.evrak_no === m.elle_tebligat; });
            const g = [['Elle bağı kaldır', function() { mazbataBagla(dosya, m.elle_tebligat, null); }]];
            if (bagli && !tebligGunuVar(bagli)) g.unshift([(m.teblig_tarihi ? 'Tebliğ tarihini değiştir (' + m.teblig_tarihi + ')' : 'Mazbatadaki tebliğ tarihini gir'),
                function() { tarihleBagla(dosya, bagli, m); }]);
            gruplar.push(g);
        }
        sagMenu(ev, gruplar);
    }

    // Belgeler (24.11.2026): bu dosyaya bağlı büro belgeleri (Armonia Yazı, belge_veri.rs). Liste kişinin
    // görebildikleri (oluşturan + bağlı işlerin kişileri + yöneticiler).
    function belgelerCiz(ic, v) {
        const k = v.kunye;
        const editor = !!window.__cuYaziBelgeAc;
        const ust = e('div', { sinif: 'i-eylem', stil: 'display:flex;gap:6px;flex-wrap:wrap;margin-bottom:8px' }, [
            e('button', { sinif: 'i-d kucuk ana', metin: '+ Yeni belge yaz', disabled: !editor, title: 'Armonia Yazı bu dosyaya bağlı açılır; Kaydet büro veritabanına yazar',
                onclick: function() { window.__cuYaziYeni({ buroDosyaId: k.id, birimAdi: k.birim, dosyaNo: k.dosya_no }); } })
        ]);
        const liste = e('div', {}, [e('div', { sinif: 'i-bilgi', metin: 'Yükleniyor…' })]);
        ic.appendChild(ust);
        ic.appendChild(liste);
        ic.appendChild(e('div', { sinif: 'alt', stil: 'margin-top:8px', metin: 'UYAP\'a göndermek için belgeyi UYAP dosya kartından açın ("UYAP\'ta aç" → "Editörde yaz" ya da belge çubuğunda "Yazı\'da aç").' }));
        istek('is/belge_liste?dosya=' + encodeURIComponent(k.id)).then(function(s) {
            bos(liste);
            if (!s.length) { liste.appendChild(e('div', { sinif: 'i-bilgi', metin: 'Bu dosyada büroya kaydedilmiş (görebildiğiniz) belge yok.' })); return; }
            s.forEach(function(b) {
                const bilgi = ['sürüm ' + b.son_surum, b.guncelleme, b.olusturan, b.imzali ? 'e-imzalı' : '', b.is_sayisi ? b.is_sayisi + ' işte' : '',
                               b.gonderim ? 'UYAP\'a gönderildi ' + b.gonderim.zaman + ' (' + b.gonderim.tur_adi + ')' : ''].filter(Boolean).join(' · ');
                liste.appendChild(e('div', { sinif: 'i-olay' }, [
                    e('div', { stil: 'display:flex;justify-content:space-between;gap:8px;align-items:center' }, [
                        e('div', { stil: 'min-width:0' }, [e('div', { stil: 'font-weight:600', metin: b.ad }), e('div', { sinif: 'alt', metin: bilgi })]),
                        e('span', { stil: 'display:flex;gap:4px;flex:none' }, [
                            e('button', { sinif: 'i-d kucuk', metin: 'Aç', disabled: !editor, title: 'Armonia Yazı\'da aç', onclick: function() { window.__cuYaziBelgeAc(b.id); } }),
                            e('button', { sinif: 'i-d kucuk tehlike', metin: 'Sil', title: 'Bütün sürümleriyle kalıcı olarak sil (geri alınamaz)', onclick: async function(ev) {
                                const dg = ev.currentTarget;
                                if (!dg.dataset.onay) { dg.dataset.onay = '1'; dg.textContent = 'Emin misiniz?'; setTimeout(function() { delete dg.dataset.onay; dg.textContent = 'Sil'; }, 4000); return; }
                                dg.disabled = true;
                                try { await istek('is/belge_sil', { id: b.id }); aktifCiz(); }
                                catch (x) { dg.disabled = false; bildir(x.message); }
                            } })
                        ])
                    ])
                ]));
            });
        }).catch(function(x) { bos(liste); liste.appendChild(e('div', { sinif: 'i-bilgi', metin: 'Belgeler alınamadı: ' + x.message })); });
    }

    function gecmisCiz(ic, g) {
        if (!g.length) { ic.appendChild(e('div', { sinif: 'i-bilgi', metin: 'Henüz kayıtlı durum değişikliği yok.' })); return; }
        g.forEach(function(o) {
            ic.appendChild(e('div', { sinif: 'i-olay' }, [
                e('div', { sinif: 'alt', metin: gunMetni(o.zaman) + o.zaman.slice(10) + ' · ' + o.tur_etiket + ' · ' + o.kaynak_etiket }),
                o.taraf ? e('div', { stil: 'font-weight:600', metin: o.taraf }) : null,
                e('div', {}, [o.eski ? e('span', { sinif: 'alt', metin: o.eski + '  →  ' }) : null, o.yeni || '—']),
                o.aciklama ? e('div', { sinif: 'alt', metin: o.aciklama }) : null
            ]));
        });
    }

    function kunyeCiz(ic, v) {
        const k = v.kunye, t = v.takip;
        const satir = function(a, b) { return e('div', { sinif: 'sat' }, [e('span', { metin: a }), e('b', { metin: b || '—' })]); };
        ic.appendChild(e('div', { sinif: 'i-kart' }, [
            satir('Birim', k.birim), satir('Dosya no', k.dosya_no), satir('Dosya türü', k.tur), satir('UYAP durumu', k.durum),
            satir('Açılış', gunMetni(k.acilis_tarihi)), satir('Son güncelleme', k.son_guncelleme)
        ]));
        ic.appendChild(e('div', { sinif: 'bolum', metin: 'Takip' }));
        const takipAdi = function(ad, kod) { return ad || (kod ? 'Kod ' + kod + ' (adı UYAP\'tan okunmadı)' : '—'); };
        ic.appendChild(e('div', { sinif: 'i-kart' }, t.okundu ? [
            satir('Takip türü', takipAdi(t.turu_ad, t.turu)),
            satir('Takip yolu', takipAdi(t.yolu_ad, t.yolu)),
            satir('Takip şekli', takipAdi(t.sekli_ad, t.sekli)),
            satir('Kesinleşme süresi', t.kesinlesme_yok || (t.kesinlesme_suresi_gun ? t.kesinlesme_suresi_gun + ' gün' + (t.kural ? ' (' + t.kural + ')' : '') : 'Tanımlı değil')),
            t.kesinlesme_yok ? null : satir('İtiraz', (t.itiraz_gun ? t.itiraz_gun + ' gün, ' : '') + (t.itiraz_durdurur === false ? 'takibi durdurmaz' : 'takibi durdurur') + (t.dayanak ? ' — ' + t.dayanak : '')),
            satir('Alacak toplamı', t.alacak_toplam), satir('Vekalet ücreti', t.vekalet_ucreti), satir('Tahsil harcı', t.tahsil_harci),
            t.okunma ? satir('UYAP\'tan okundu', gunMetni(t.okunma) + t.okunma.slice(10))
                     : e('div', { sinif: 'alt', stil: 'margin-top:6px', metin: 'Adlar için "UYAP\'tan güncelle" ile takip türünü yeniden okuyun.' })
        ] : [e('div', { sinif: 'alt', metin: k.talimat ? 'Talimat dosyası: kesinleşme yok (esas dosyada izlenir); takip türü sorulmaz.'
                                                       : 'Takip türü henüz UYAP\'tan okunmadı. Yukarıdaki "UYAP\'tan güncelle" ile alınır.' })]));
        // İşlem durumu (büronun kendi işareti)
        const sec = e('select', {}, [['aktif', 'Aktif - işlem yapılabilir'], ['pasif', 'Pasif - işlem yapma'], ['beklemede', 'Beklemede']].map(function(o) {
            const op = e('option', { value: o[0], metin: o[1] }); if (k.islem_durumu === o[0]) op.selected = true; return op;
        }));
        const not = e('input', { type: 'text', placeholder: 'Not / gerekçe', value: k.islem_durumu_notu || '', stil: 'flex:1;min-width:140px' });
        const bitis = e('input', { type: 'date', value: k.bekleme_bitis || '', title: 'Bekleme bitişi (yalnız Beklemede)' });
        ic.appendChild(e('div', { sinif: 'bolum', metin: 'İşlem durumu' }));
        ic.appendChild(e('div', { sinif: 'i-form' }, [sec, bitis, not, e('button', { sinif: 'i-d ana', metin: 'Kaydet', onclick: async function() {
            try { await istek('islem_durumu', { dosya: k.id, durum: sec.value, notu: not.value, bekleme_bitis: bitis.value || null }); bildir('Kaydedildi.'); seciliyiTazele(); }
            catch (x) { bildir(x.message); }
        } })]));
    }

    // ── Dosya Listesi (20.11.2026; kullanıcı notu Desktop\süreçler\dosya listesi sekmesi.md) ──
    // Dosya başına bir satır (src/icra_dosyalar.rs). Başlığa tıklayınca küçük süzgeç penceresi açılır: metin kutusu
    // (yazınca içinde geçenler), sıralama ve onay kutulu değerler (Türkçe alfabetik, sayılar sayı olarak). İcra
    // dairesinde "Tümünü seç"in altında daire grupları (Banka, Abonelik, Merkezi Takip, Gayrimenkul Satış): adında o
    // kelime geçen daireleri birlikte seçer / çıkarır (kullanıcı kararı 20.11.2026: ayrı hızlı filtre yok).
    // Süzgeç dili sunucuyla aynı: düz yazı = içinde geçen, "=" + değerler = seçilenler, "!" + değerler = hariç.
    // Düğmeler toplu taramayı (icra_guncelle.rs) başlatır; takip türü ve taraf güncellemesi yalnız süzülen dosyalara.
    const DL_SUTUN = [
        { ad: 'İcra Dairesi', k: 'birim', s: 'birim' },
        { ad: 'Dosya No', k: 'dosya_no', s: 'dosya_no' },
        // UYAP dosya türü: İcra Dosyası, İcra Dosyası (MTS'den), MTS, Talimat Dosyası, İflas Dosyası … (21.11.2026)
        { ad: 'Dosya Türü', k: 'dosya_tur', s: 'dosya_tur' },
        { ad: 'Alacaklı', k: 'alacakli', s: 'alacakli' },
        { ad: 'Borçlu', k: 'borclu', s: 'borclu' },
        { ad: 'Rolüm', k: 'rol', s: 'rol', kodlu: true },
        { ad: 'Takip Türü', k: 'takip_turu', s: 'takip_turu' },
        { ad: 'Takip Yolu', k: 'takip_yolu', s: 'takip_yolu' },
        { ad: 'Takip Şekli', k: 'takip_sekli', s: 'takip_sekli' },
        { ad: 'Açılış', s: 'acilis', tarih: true },
        { ad: 'Kapanış', s: 'kapanis', tarih: true, kapali: true }
    ];
    // trKucuk: Türkçe harfler sadeleşmiş küçük harf ("satış" → "satis").
    const DL_GRUPLAR = [
        ['Banka', function(m) { return m.indexOf('banka') >= 0; }],
        ['Abonelik', function(m) { return m.indexOf('abonelik') >= 0; }],
        ['Merkezi Takip (MTS)', function(m) { return m.indexOf('merkezi takip') >= 0 || /(^|[^a-z])mts([^a-z]|$)/.test(m); }],
        ['Gayrimenkul Satış', function(m) { return m.indexOf('gayrimenkul') >= 0 || m.indexOf('satis') >= 0; }]
    ];
    const DL_ROL_RENK = { alacakli: 'var(--i-vurgu-koyu)', borclu: '#c06a2f', diger_alacakli: '#7a5fb0', ucuncu: '#3f7fa3', diger: '#8a877e' };
    const DL_DURUMLAR = [['acik', 'Açık'], ['kapali', 'Kapalı'], ['', 'Tümü']];
    const trSirala = new Intl.Collator('tr', { numeric: true, sensitivity: 'base' });
    // Sütunu olmayan kodlu süzgeçler: dosya yılı (Dosya No süzgecinin içinde) ve açılış / kapanış gününün yılı ve ayı
    // (tarih kutularının yanındaki "Yıl / ay" seçimi). Değerler: yıl "2025", ay "1"…"12"; birden çok seçim satır sonuyla.
    const DL_YIL = { ad: 'Dosya yılı', k: 'yil', kodlu: true };
    const DL_YILAY = {
        acilis_yil: { ad: 'Açılış yılı', k: 'acilis_yil', kodlu: true }, acilis_ay: { ad: 'Açılış ayı', k: 'acilis_ay', kodlu: true },
        kapanis_yil: { ad: 'Kapanış yılı', k: 'kapanis_yil', kodlu: true, kapali: true }, kapanis_ay: { ad: 'Kapanış ayı', k: 'kapanis_ay', kodlu: true, kapali: true }
    };
    const AY_ADLARI = ['Ocak', 'Şubat', 'Mart', 'Nisan', 'Mayıs', 'Haziran', 'Temmuz', 'Ağustos', 'Eylül', 'Ekim', 'Kasım', 'Aralık'];
    // Sıralanabilir süzgeçler (Süzgeçler penceresi): sütunlar + yıl + yıl / ay.
    const DL_SUZGECLER = DL_SUTUN.filter(function(c) { return c.k; }).concat([DL_YIL, DL_YILAY.acilis_yil, DL_YILAY.acilis_ay, DL_YILAY.kapanis_yil, DL_YILAY.kapanis_ay]);
    function dlFiltreBos() {
        const f = { durum: 'acik', acilis_bas: '', acilis_son: '', kapanis_bas: '', kapanis_son: '', sirala: '', yon: 'artan', sayfa: 1, sira: [] };
        DL_SUZGECLER.forEach(function(c) { f[c.k] = ''; });
        return f;
    }
    // acik: süzgeç penceresi açık sütun (k ya da s; "acilis_ya" / "kapanis_ya": yıl-ay penceresi; "suzgecler": süzgeç
    // sırası); yazi: penceredeki metin; onay: başlatılmayı bekleyen güncelleme.
    d.dl = { filtre: dlFiltreBos(), liste: null, acik: null, yazi: '', onay: null, cizimImzasi: '',
             sutun: DL_SUTUN, suzgecler: DL_SUZGECLER, bos: dlFiltreBos };
    // Süzgeç işlevleri (başlık pencereleri, Süzgeçler, yıl / ay, sıralama) etkin sekmenin listesiyle çalışır: Dosya
    // Listesi (d.dl) ya da Kesinleştirme (d.ks; 22.11.2026). Her liste kendi sütunlarını, süzgeçlerini ve boş süzgecini taşır.
    function D() { return d.sekme === 'kesin' ? d.ks : d.dl; }
    let dlAraZaman = null;
    // Süzgeç uygulanıyor mu (kapanış süzgeçleri yalnız "Kapalı"da).
    function dlEtkin(c) { const f = D().filtre; return !!f[c.k] && !(c.kapali && f.durum !== 'kapali'); }
    // Süzgeç sırası: yeni konan süzgeç sona eklenir, kaldırılan çıkar (sunucu: bir süzgecin seçenekleri yalnız
    // kendinden önceki süzgeçlere uyan dosyalardan gelir).
    function dlSiraEsitle() {
        const f = D().filtre;
        f.sira = (f.sira || []).filter(function(k, i, a) { const c = D().suzgecler.find(function(x) { return x.k === k; }); return c && dlEtkin(c) && a.indexOf(k) === i; });
        D().suzgecler.forEach(function(c) { if (dlEtkin(c) && f.sira.indexOf(c.k) < 0) f.sira.push(c.k); });
        return f.sira;
    }
    function dlSorgusu(sayfasiz) {
        const f = D().filtre, s = {};
        ['durum', 'acilis_bas', 'acilis_son', 'sirala', 'yon'].forEach(function(k) { if (f[k]) s[k] = f[k]; });
        if (f.durum === 'kapali') ['kapanis_bas', 'kapanis_son'].forEach(function(k) { if (f[k]) s[k] = f[k]; });
        D().suzgecler.forEach(function(c) { if (dlEtkin(c)) s[c.k] = f[c.k]; });
        const sira = dlSiraEsitle();
        if (sira.length > 1) s.sira = sira.join(',');
        if (!sayfasiz) { s.sayfa = String(f.sayfa); s.boyut = '100'; }
        if (d.secListe) s.liste = String(d.secListe);
        return new URLSearchParams(s).toString();
    }
    function dlSutunBul(id) { return D().sutun.find(function(c) { return (c.k || c.s) === id; }); }
    // Sütun süzgecinin hâli: hariç mi, listedeki değerler, düz yazı.
    function dlSecim(c) {
        const v = D().filtre[c.k] || '';
        if (v.charAt(0) === '!') { const r = v.slice(1); return { haric: true, liste: r === '' ? [] : r === '\n' ? [''] : r.split('\n'), yazi: '' }; }
        if (c.kodlu) return { haric: false, liste: v ? v.split('\n') : [], yazi: '' };
        if (v.charAt(0) === '=') return { haric: false, liste: v.slice(1).split('\n'), yazi: '' };
        return { haric: false, liste: [], yazi: v };
    }
    function dlAcikMi(sec, deger) { return sec.haric ? sec.liste.indexOf(deger) < 0 : sec.liste.indexOf(deger) >= 0; }
    function dlUygula(k, v) {
        clearTimeout(dlAraZaman);
        D().filtre[k] = v;
        D().filtre.sayfa = 1;
        yukle();
    }
    // Değerleri işaretler (ac) ya da işaretini kaldırır. "Hariç" kipte işaretsiz = hariç listesinde.
    function dlDegistir(c, degerler, ac) {
        const sec = dlSecim(c);
        const liste = sec.liste.slice();
        const listede = ac !== sec.haric;
        degerler.forEach(function(x) {
            const i = liste.indexOf(x);
            if (listede && i < 0) liste.push(x);
            if (!listede && i >= 0) liste.splice(i, 1);
        });
        let v;
        if (sec.haric) v = '!' + (liste.length === 1 && liste[0] === '' ? '\n' : liste.join('\n'));
        else v = liste.length ? (c.kodlu ? '' : '=') + liste.join('\n') : '';
        dlUygula(c.k, v);
    }
    function huni() {
        const ns = 'http://www.w3.org/2000/svg';
        const s = document.createElementNS(ns, 'svg');
        [['width', '13'], ['height', '13'], ['viewBox', '0 0 24 24'], ['fill', 'none'], ['stroke', 'currentColor'], ['stroke-width', '2'],
         ['stroke-linecap', 'round'], ['stroke-linejoin', 'round'], ['aria-hidden', 'true']].forEach(function(a) { s.setAttribute(a[0], a[1]); });
        const p = document.createElementNS(ns, 'path');
        p.setAttribute('d', 'M3 5h18l-7 8v6l-4 2v-8z');
        s.appendChild(p);
        return s;
    }
    function dlAcilirKaldir() { if (!kok) return; const a = kok.querySelector('.i-dlp'); if (a) a.remove(); }
    function dlKapat() { clearTimeout(dlAraZaman); D().acik = null; D().yazi = ''; dlAcilirKaldir(); }
    // c: sütun ya da pencere kimliği ("acilis_ya", "kapanis_ya", "suzgecler").
    function dlAc(c) {
        const id = typeof c === 'string' ? c : (c.k || c.s);
        if (D().acik === id) { dlKapat(); return; }
        D().acik = id;
        D().yazi = typeof c !== 'string' && c.k ? dlSecim(c).yazi : '';
        dlAcilirCiz(true);
    }
    // Penceredeki değer listesi: Türkçe alfabetik; yazılan metne göre daralır.
    function dlListeDoldur(c, liste) {
        bos(liste);
        const l = D().liste;
        const sec = dlSecim(c);
        const hepsi = ((l && l.secenekler && l.secenekler[c.k]) || []).slice();
        sec.liste.forEach(function(x) {
            if (!hepsi.some(function(o) { return o.deger === x; })) hepsi.push({ deger: x, etiket: x || '(boş)', adet: null });
        });
        hepsi.sort(function(a, b) { return trSirala.compare(a.etiket, b.etiket); });
        const aranan = trKucuk(D().yazi).split(/\s+/).filter(Boolean);
        const gorunen = hepsi.filter(function(o) { const m = trKucuk(o.etiket); return aranan.every(function(a) { return m.indexOf(a) >= 0; }); });
        const kutucuk = function(on, ad, adet, fn, sinif, pasif) {
            const k = e('input', { type: 'checkbox', disabled: !!pasif });
            k.checked = on;
            k.addEventListener('change', fn);
            return e('label', { sinif: (sinif || '') + (pasif ? ' pasif' : '') },
                [k, e('span', { sinif: 'ad', metin: ad }), adet == null ? null : e('span', { sinif: 'adet', metin: String(adet) })]);
        };
        if (!aranan.length && hepsi.length > 1) {
            const tumu = sec.haric && !sec.liste.length;
            liste.appendChild(kutucuk(tumu, 'Tümünü seç', null, function() { dlUygula(c.k, tumu ? '' : '!'); }, 'grup'));
            if (c.k === 'birim') {
                DL_GRUPLAR.forEach(function(gr) {
                    const uyanlar = hepsi.filter(function(o) { return gr[1](trKucuk(o.deger)); }).map(function(o) { return o.deger; });
                    const on = uyanlar.length > 0 && uyanlar.every(function(x) { return dlAcikMi(sec, x); });
                    liste.appendChild(kutucuk(on, gr[0], uyanlar.length ? uyanlar.length + ' daire' : 'yok',
                        function() { dlDegistir(c, uyanlar, !on); }, 'grup', !uyanlar.length));
                });
            }
            liste.appendChild(e('div', { sinif: 'ayrac' }));
        }
        gorunen.forEach(function(o) {
            const on = dlAcikMi(sec, o.deger);
            liste.appendChild(kutucuk(on, o.etiket, o.adet, function() { dlDegistir(c, [o.deger], !on); }));
        });
        if (!gorunen.length) liste.appendChild(e('div', { sinif: 'yok', metin: hepsi.length ? 'Yazdığınıza uyan değer yok.' : 'Bu ölçütlerde değer yok.' }));
        const sinir = l && l.secenek_siniri;
        if (sinir && hepsi.length >= sinir) liste.appendChild(e('div', { sinif: 'yok', metin: 'En çok kullanılan ' + sinir + ' değer gösteriliyor; diğerleri için yazın.' }));
    }
    // Pencereyi düğmenin altına yerleştirir (ekrana sığdırarak).
    function dlKonumla(kutu, dugme, giris, odakla) {
        kok.appendChild(kutu);
        const r = dugme.getBoundingClientRect(), g = kutu.getBoundingClientRect();
        kutu.style.left = Math.max(8, Math.min(r.left, window.innerWidth - g.width - 8)) + 'px';
        kutu.style.top = (r.bottom + 4) + 'px';
        const altta = window.innerHeight - r.bottom - 12;
        if (g.height > altta) { kutu.style.maxHeight = Math.max(220, altta) + 'px'; kutu.style.overflow = 'auto'; }
        if (giris && odakla) { giris.focus(); if (giris.setSelectionRange) giris.setSelectionRange(giris.value.length, giris.value.length); }
    }
    // Kodlu değerlerin düğme ızgarası (yıllar, aylar): basınca seçilir / seçim kalkar; adet küçük yazıyla.
    function dlIzgara(c, degerler, etiket) {
        const sec = dlSecim(c);
        return e('div', { sinif: 'izgara', role: 'group', 'aria-label': c.ad }, degerler.map(function(o) {
            const on = sec.liste.indexOf(o.deger) >= 0 && !sec.haric;
            return e('button', { type: 'button', sinif: 'i-d kucuk' + (on ? ' on' : '') + (o.adet === 0 ? ' bos' : ''), 'aria-pressed': on ? 'true' : 'false',
                title: (etiket ? etiket(o.deger) : o.deger) + (o.adet != null ? ' · ' + sayi(o.adet) + ' dosya' : ''),
                onclick: function() { dlDegistir(c, [o.deger], !on); } },
                [e('span', { metin: etiket ? etiket(o.deger) : o.deger }), o.adet != null ? e('small', { metin: sayi(o.adet) }) : null]);
        }));
    }
    function dlSecenekAdet(k) {
        const m = {};
        ((D().liste && D().liste.secenekler && D().liste.secenekler[k]) || []).forEach(function(o) { m[o.deger] = o.adet; });
        return m;
    }
    // Yıllar: seçenekler (adetli) + seçili olup listede kalmayanlar; yeniden eskiye.
    function dlYillar(c) {
        const adet = dlSecenekAdet(c.k);
        const yillar = Object.keys(adet).filter(function(y) { return y; });
        dlSecim(c).liste.forEach(function(y) { if (y && yillar.indexOf(y) < 0) yillar.push(y); });
        yillar.sort(function(a, b) { return Number(b) - Number(a); });
        return yillar.map(function(y) { return { deger: y, adet: adet[y] == null ? 0 : adet[y] }; });
    }
    function ayEtiketi(a) { return AY_ADLARI[Number(a) - 1] || a; }
    // "Yıl / ay" penceresi (açılış ya da kapanış): önce yıllar, sonra aylar; yalnız ay da seçilebilir (her yılın o ayı).
    function dlYilAyCiz(onEk, dugme, odakla) {
        const cy = DL_YILAY[onEk + '_yil'], ca = DL_YILAY[onEk + '_ay'];
        const ad = onEk === 'acilis' ? 'Açılış' : 'Kapanış';
        const kutu = e('div', { sinif: 'i-dlp i-dlp-genis', role: 'dialog', 'aria-label': ad + ' yılı ve ayı' });
        const yillar = dlYillar(cy);
        const ayAdet = dlSecenekAdet(ca.k);
        kutu.appendChild(e('div', { sinif: 'bolum' }, [e('b', { metin: ad + ' yılı' }),
            yillar.length ? dlIzgara(cy, yillar) : e('div', { sinif: 'yok', metin: 'Bu ölçütlerde ' + ad.toLocaleLowerCase('tr-TR') + ' günü olan dosya yok.' })]));
        kutu.appendChild(e('div', { sinif: 'bolum' }, [e('b', { metin: ad + ' ayı' }),
            dlIzgara(ca, AY_ADLARI.map(function(x, i) { const k = String(i + 1); return { deger: k, adet: ayAdet[k] == null ? 0 : ayAdet[k] }; }), ayEtiketi),
            e('div', { sinif: 'yok', metin: 'Yıl seçmeden ay seçerseniz her yılın o ayı gelir (örn. bütün Mart ayları).' })]));
        const f = D().filtre;
        kutu.appendChild(e('div', { sinif: 'alt-cubuk' }, [
            e('button', { sinif: 'i-dahasi', type: 'button', metin: 'Temizle', disabled: !(f[cy.k] || f[ca.k]),
                onclick: function() { f[cy.k] = ''; f[ca.k] = ''; f.sayfa = 1; yukle(); } }),
            e('button', { sinif: 'i-d ana', type: 'button', metin: 'Tamam', stil: 'padding:5px 16px', onclick: dlKapat })
        ]));
        dlKonumla(kutu, dugme, kutu.querySelector('button'), odakla);
    }
    // Etkin süzgeçlerin okunur özeti: [{k, ad, metin}] (sırayla). Tarih aralıkları sabit en üstte (sıralanmaz).
    function dlDegerEtiketi(c, deger) {
        if (c.k === 'acilis_ay' || c.k === 'kapanis_ay') return deger ? ayEtiketi(deger) : '(tarihsiz)';
        const o = ((D().liste && D().liste.secenekler && D().liste.secenekler[c.k]) || []).find(function(x) { return x.deger === deger; });
        return o ? o.etiket : (deger || '(boş)');
    }
    function dlSuzgecOzeti(c) {
        const sec = dlSecim(c);
        if (sec.yazi) return '“' + sec.yazi + '” geçen';
        const adlar = sec.liste.map(function(x) { return dlDegerEtiketi(c, x); });
        const metin = adlar.join(', ');
        const kisa = metin.length > 60 ? adlar[0] + ' ve ' + (adlar.length - 1) + ' değer daha' : metin;
        return sec.haric ? (adlar.length ? 'tümü, hariç: ' + kisa : 'tümü') : kisa;
    }
    function dlTarihSuzgecleri() {
        const f = D().filtre, out = [];
        [['acilis', 'Açılış tarihi'], ['kapanis', 'Kapanış tarihi']].forEach(function(a) {
            if (a[0] === 'kapanis' && f.durum !== 'kapali') return;
            const b = f[a[0] + '_bas'], s = f[a[0] + '_son'];
            if (b || s) out.push({ ad: a[1], metin: (b ? gunMetni(b) : '…') + ' – ' + (s ? gunMetni(s) : '…'),
                kaldir: function() { f[a[0] + '_bas'] = ''; f[a[0] + '_son'] = ''; } });
        });
        return out;
    }
    function dlSuzgecKaldir(k) { const f = D().filtre; f[k] = ''; f.sira = (f.sira || []).filter(function(x) { return x !== k; }); f.sayfa = 1; yukle(); }
    function dlSiraTasi(k, hedef) {
        const sira = dlSiraEsitle().slice();
        const i = sira.indexOf(k);
        if (i < 0 || hedef < 0 || hedef >= sira.length || hedef === i) return;
        sira.splice(i, 1);
        sira.splice(hedef, 0, k);
        D().filtre.sira = sira;
        yukle();
    }
    // Süzgeçler penceresi: etkin süzgeçler sırayla; ✕ ile kaldırılır, sürükleyerek (ya da ↑ ↓) yeri değişir. Üstteki
    // süzgeç alttakini kapsar: alttakinin seçenekleri yalnız üstündekilere uyan dosyalardan gelir.
    function dlSuzgeclerCiz(dugme, odakla) {
        const kutu = e('div', { sinif: 'i-dlp i-dlp-genis', role: 'dialog', 'aria-label': 'Süzgeçler' });
        const tarihler = dlTarihSuzgecleri();
        const sira = dlSiraEsitle();
        kutu.appendChild(e('div', { sinif: 'yok', stil: 'padding:0 2px', metin: sira.length > 1
            ? 'Üstteki süzgeç alttakini kapsar: bir süzgeç açıldığında seçenekleri yalnız üstündeki süzgeçlere uyan dosyalardan gelir. Yerini değiştirmek için sürükleyin.'
            : 'Sütun başlıklarından süzgeç koydukça burada sırayla listelenir.' }));
        const liste = e('div', { sinif: 'sira', role: 'list' });
        tarihler.forEach(function(t) {
            liste.appendChild(e('div', { sinif: 'sat sabit', role: 'listitem', title: 'Tarih aralığı her zaman en üstte uygulanır' }, [
                e('span', { sinif: 'tut', metin: '•', 'aria-hidden': 'true' }), e('span', { sinif: 'ad' }, [e('span', { metin: t.ad + ': ' }), e('b', { metin: t.metin })]),
                e('button', { type: 'button', sinif: 'x', metin: '✕', title: t.ad + ' süzgecini kaldır', 'aria-label': t.ad + ' süzgecini kaldır',
                    onclick: function() { t.kaldir(); D().filtre.sayfa = 1; yukle(); } })]));
        });
        let surukle = null;
        sira.forEach(function(k, i) {
            const c = D().suzgecler.find(function(x) { return x.k === k; });
            const sat = e('div', { sinif: 'sat', role: 'listitem', draggable: 'true', 'data-k': k }, [
                e('span', { sinif: 'tut', metin: '⋮⋮', title: 'Sürükleyerek yerini değiştirin', 'aria-hidden': 'true' }),
                e('span', { sinif: 'no', metin: String(i + 1) + '.' }),
                e('span', { sinif: 'ad' }, [e('span', { metin: c.ad + ': ' }), e('b', { metin: dlSuzgecOzeti(c) })]),
                e('button', { type: 'button', sinif: 'ok', metin: '↑', disabled: i === 0, title: 'Yukarı taşı', 'aria-label': c.ad + ' süzgecini yukarı taşı',
                    onclick: function() { dlSiraTasi(k, i - 1); } }),
                e('button', { type: 'button', sinif: 'ok', metin: '↓', disabled: i === sira.length - 1, title: 'Aşağı taşı', 'aria-label': c.ad + ' süzgecini aşağı taşı',
                    onclick: function() { dlSiraTasi(k, i + 1); } }),
                e('button', { type: 'button', sinif: 'x', metin: '✕', title: c.ad + ' süzgecini kaldır', 'aria-label': c.ad + ' süzgecini kaldır',
                    onclick: function() { dlSuzgecKaldir(k); } })
            ]);
            sat.addEventListener('dragstart', function(ev) { surukle = k; sat.classList.add('tasiniyor'); try { ev.dataTransfer.setData('text/plain', k); ev.dataTransfer.effectAllowed = 'move'; } catch (x) {} });
            sat.addEventListener('dragend', function() { surukle = null; sat.classList.remove('tasiniyor'); });
            sat.addEventListener('dragover', function(ev) { if (surukle && surukle !== k) { ev.preventDefault(); sat.classList.add('hedef'); } });
            sat.addEventListener('dragleave', function() { sat.classList.remove('hedef'); });
            sat.addEventListener('drop', function(ev) { ev.preventDefault(); sat.classList.remove('hedef'); if (surukle && surukle !== k) dlSiraTasi(surukle, i); });
            liste.appendChild(sat);
        });
        if (!tarihler.length && !sira.length) liste.appendChild(e('div', { sinif: 'yok', metin: 'Süzgeç yok.' }));
        kutu.appendChild(liste);
        kutu.appendChild(e('div', { sinif: 'alt-cubuk' }, [
            e('button', { sinif: 'i-dahasi', type: 'button', metin: 'Tümünü kaldır', disabled: !(tarihler.length || sira.length), onclick: dlTumunuTemizle }),
            e('button', { sinif: 'i-d ana', type: 'button', metin: 'Tamam', stil: 'padding:5px 16px', onclick: dlKapat })
        ]));
        dlKonumla(kutu, dugme, kutu.querySelector('.sat button, .alt-cubuk .ana'), odakla);
    }
    // Bütün süzgeçleri kaldırır (durum ve sıralama kalır).
    function dlTumunuTemizle() {
        const L = D(), f = L.filtre;
        L.filtre = L.bos();
        Object.assign(L.filtre, { sirala: f.sirala, yon: f.yon });
        if (f.durum !== undefined) L.filtre.durum = f.durum;
        dlKapat();
        yukle();
    }
    function dlAcilirCiz(odakla) {
        dlAcilirKaldir();
        const id = D().acik;
        const dugme = id && kok && kok.querySelector('[data-dl="' + id + '"]');
        if (!dugme) { D().acik = null; return; }
        if (id === 'acilis_ya' || id === 'kapanis_ya') { dlYilAyCiz(id.replace('_ya', ''), dugme, odakla); return; }
        if (id === 'suzgecler') { dlSuzgeclerCiz(dugme, odakla); return; }
        const c = dlSutunBul(id);
        if (!c) { D().acik = null; return; }
        const f = D().filtre;
        const kutu = e('div', { sinif: 'i-dlp', role: 'dialog', 'aria-label': c.ad + ' süzgeci' });
        let giris = null, liste = null;
        // Yazıyla süzme: liste seçimi yoksa tablo da süzülür; seçim varsa yazı yalnız listeyi daraltır.
        const yaziUygula = function() {
            if (!giris) return false;
            const sec = dlSecim(c);
            if (sec.haric || sec.liste.length || giris.value.trim() === (f[c.k] || '')) return false;
            dlUygula(c.k, giris.value.trim());
            return true;
        };
        if (c.k && !c.kodlu) {
            giris = e('input', { type: 'text', autocomplete: 'off', spellcheck: 'false', value: D().yazi,
                placeholder: c.k === 'birim' ? 'Örn. ankara, istanbul 2' : 'Yazarak süzün…', 'aria-label': c.ad + ' için yazarak süz' });
            giris.addEventListener('input', function() {
                D().yazi = giris.value;
                if (liste) dlListeDoldur(c, liste);
                clearTimeout(dlAraZaman);
                dlAraZaman = setTimeout(yaziUygula, 350);
            });
            giris.addEventListener('keydown', function(ev) {
                if (ev.key === 'Escape') { ev.preventDefault(); dlKapat(); }
                else if (ev.key === 'Enter') { ev.preventDefault(); clearTimeout(dlAraZaman); if (!yaziUygula()) dlKapat(); }
            });
            kutu.appendChild(giris);
        }
        const sir = function(yon, ad) {
            return e('button', { sinif: 'i-d' + (f.sirala === c.s && f.yon === yon ? ' on' : ''), type: 'button', metin: ad,
                onclick: function() { f.sirala = c.s; f.yon = yon; f.sayfa = 1; yukle(); } });
        };
        kutu.appendChild(e('div', { sinif: 'sirala' }, c.tarih ? [sir('artan', 'Eskiden yeniye'), sir('azalan', 'Yeniden eskiye')]
                                                                : [sir('artan', 'A → Z'), sir('azalan', 'Z → A')]));
        // Dosya No: yıla göre süzme (UYAP'taki gibi; birden çok yıl seçilebilir).
        const yilVar = c.k === 'dosya_no';
        if (yilVar) {
            const yillar = dlYillar(DL_YIL);
            kutu.appendChild(e('div', { sinif: 'bolum' }, [e('b', { metin: 'Dosya yılı' }),
                yillar.length ? dlIzgara(DL_YIL, yillar) : e('div', { sinif: 'yok', metin: 'Bu ölçütlerde dosya yok.' })]));
        }
        if (c.k) { liste = e('div', { sinif: 'liste' }); kutu.appendChild(liste); dlListeDoldur(c, liste); }
        else {
            // Tarih sütunları: aralık ve yıl / ay seçimi üstteki süzgeç alanında. Sütunun kendi notu varsa (ör.
            // Kesinleştirme'de kesinleşme günü: yalnız sıralama) yalnız o yazılır.
            if (c.not) kutu.appendChild(e('div', { sinif: 'yok', metin: c.not }));
            else {
                const onEk = c.kapali ? 'kapanis' : 'acilis';
                kutu.appendChild(e('div', { sinif: 'yok', metin: c.kapali ? 'Kapanış tarihi aralığını ya da yılını / ayını yukarıdan seçin ("Kapalı" dosyalarda).'
                                                                         : 'Tarih aralığını yukarıdaki "Açılış tarihi" kutularından seçin.' }));
                kutu.appendChild(e('button', { sinif: 'i-d', type: 'button', metin: 'Yıl / ay seç…',
                    onclick: function() { D().acik = onEk + '_ya'; dlAcilirCiz(true); } }));
            }
        }
        const temizlenecek = !!(c.k && f[c.k]) || f.sirala === c.s || (yilVar && !!f.yil);
        kutu.appendChild(e('div', { sinif: 'alt-cubuk' }, [
            e('button', { sinif: 'i-dahasi', type: 'button', metin: 'Temizle', disabled: !temizlenecek,
                onclick: function() {
                    if (c.k) f[c.k] = '';
                    if (yilVar) f.yil = '';
                    if (f.sirala === c.s) { f.sirala = ''; f.yon = 'artan'; }
                    D().yazi = '';
                    f.sayfa = 1;
                    yukle();
                } }),
            e('button', { sinif: 'i-d ana', type: 'button', metin: 'Tamam', stil: 'padding:5px 16px',
                onclick: function() { clearTimeout(dlAraZaman); yaziUygula(); dlKapat(); } })
        ]));
        dlKonumla(kutu, dugme, giris, odakla);
    }
    // Pencere dışına tıklayınca kapanır (açan düğmeye basmak da kapatır: düğmenin kendi işi).
    document.addEventListener('mousedown', function(ev) {
        if (!D().acik || !kok || !ev.target.closest) return;
        if (ev.target.closest('.i-dlp') || ev.target.closest('#cu-icra [data-dl]')) return;
        dlKapat();
    }, true);

    // Toplu tarama. Önce tahmin gösterilir (kaç dosyaya, kaç istek, ne kadar sürer); kullanıcı "Başlat" deyince gider.
    // "liste": bütün açık dosyalar (süzgeç uygulanmaz). "takip" / "taraf": süzgeçteki dosyalardan yalnız eksiği olanlar.
    // "hepsi": dosya listesi + süzgeçteki açık dosyaların eksik tarafları ve takip türleri (yeni dosyalar hep dahil).
    const DL_TURLER = {
        hepsi: 'Tüm verileri güncelle', liste: 'Dosya listesini güncelle', takip: 'Takip türlerini güncelle', taraf: 'Tarafları güncelle'
    };
    function sayi(n) { return Number(n || 0).toLocaleString('tr-TR'); }
    function sureYazisi(sn) {
        if (sn < 60) return '1 dakikadan az';
        const dk = Math.round(sn / 60);
        return dk < 60 ? '≈ ' + dk + ' dk' : '≈ ' + Math.floor(dk / 60) + ' sa ' + (dk % 60) + ' dk';
    }
    // "Tüm verileri güncelle"de iki tahmin alınır: dosya listesiyle (hepsi) ve listesiz, yalnız eksikler (eksik);
    // kullanıcıya dosya listesinin en son ne zaman alındığı söylenip listenin yeniden alınıp alınmayacağı sorulur
    // (kullanıcı isteği 21.11.2026: her seferinde baştan liste çekilmesin).
    async function dlGuncelle(mod) {
        if (gtSuruyor()) { d.gtAcik = true; gtCiz(); return; }
        try {
            const sorgu = 'dosyalar_istek?' + dlSorgusu(true) + '&mod=';
            if (mod === 'hepsi') {
                const r = await Promise.all([istek(sorgu + 'hepsi'), istek(sorgu + 'eksik')]);
                d.dl.onay = { mod: mod, ad: DL_TURLER[mod], t: r[0], eksik: r[1] };
            } else {
                d.dl.onay = { mod: mod, ad: DL_TURLER[mod], t: await istek(sorgu + mod) };
            }
        } catch (x) { bildir(x.message); return; }
        dlCiz();
    }
    // "eksik": Tüm verileri güncelle, dosya listesi yeniden alınmadan (yalnız süzgeçteki açık dosyaların eksikleri;
    // sunucuda "hepsi" + seçili dosyalar, listedeki sırayla).
    async function dlBaslat(mod) {
        d.dl.onay = null;
        const istekGovdesi = { mod: mod === 'eksik' ? 'hepsi' : mod };
        try {
            if (mod !== 'liste') {
                const idler = await istek('dosyalar_kimlikleri?' + dlSorgusu(true) + '&mod=' + mod);
                if (mod === 'hepsi') istekGovdesi.hedefler = idler;
                else if (!idler.length) { bildir('Süzgeçteki dosyalarda eksik yok; istek gerekmiyor.'); dlCiz(); return; }
                else istekGovdesi.dosyalar = idler;
            }
            d.gt = await gtIstek('', istekGovdesi);
            d.gtAcik = true;
            gtYokla();
        } catch (x) { bildir(x.message); }
        if (d.sekme === 'dosyalar') dlCiz();
    }
    // Dosya listesinin en son alındığı zaman: (metin, kaç saat önce | null).
    function listeZamaniBilgisi(z) {
        if (!z || !z.zaman) return { metin: 'Dosya listesi bu bilgisayarda henüz hiç alınmadı.', saat: null };
        const t = new Date(z.zaman);
        if (isNaN(t.getTime())) return { metin: 'Dosya listesinin en son alındığı zaman okunamadı.', saat: null };
        const dk = Math.max(0, Math.round((Date.now() - t.getTime()) / 60000));
        const once = dk < 1 ? 'az önce' : dk < 60 ? dk + ' dakika önce' : dk < 48 * 60 ? Math.round(dk / 60) + ' saat önce' : Math.round(dk / 1440) + ' gün önce';
        const saat = String(t.getHours()).padStart(2, '0') + ':' + String(t.getMinutes()).padStart(2, '0');
        const gun = gunMetni(z.zaman.slice(0, 10));
        const metin = z.yaklasik
            ? 'Dosyalar en son ' + gun + ' ' + saat + '\'te (' + once + ') güncellendi; dosya listesinin tam alındığı zaman kayıtlı değil.'
            : 'Dosya listesi en son ' + gun + ' ' + saat + '\'te (' + once + ') alındı' + (z.alinan != null ? ': ' + sayi(z.alinan) + ' açık dosya.' : '.');
        return { metin: metin, saat: dk / 60 };
    }
    // Tüm verileri güncelle: liste yeniden alınsın mı? İki seçenek, tahminleriyle; liste yakın zamanda (24 saat içinde)
    // alındıysa "Hayır" öne çıkar.
    function dlHepsiOnayi(o) {
        const t = o.t, x = o.eksik;
        const z = listeZamaniBilgisi(t.liste_zamani);
        const yakin = z.saat != null && z.saat < 24 && !(t.liste_zamani && t.liste_zamani.yaklasik);
        const eksikYok = !x || !x.hedef;
        const secenek = function(baslik, aciklama, dugme) {
            return e('div', { sinif: 'i-gt-secenek' }, [e('div', {}, [e('b', { metin: baslik }), e('div', { sinif: 'i-gt-not', metin: aciklama })]), dugme]);
        };
        return e('div', { sinif: 'i-gt', role: 'region', 'aria-label': o.ad }, [
            e('div', { stil: 'font-weight:600;margin-bottom:6px', metin: o.ad }),
            e('div', { sinif: 'i-gt-not', stil: 'color:var(--i-yazi)', metin: z.metin }),
            e('div', { stil: 'font-weight:600;margin:8px 0 6px', metin: 'Dosya listesi de yeniden alınsın mı?' }),
            secenek('Evet, listeyi de güncelle',
                'Yeni ve kapanan dosyalar da bulunur. Ardından süzgeçteki ' + sayi(t.dosya) + ' dosyadan ' + sayi(t.hedef) + ' açık dosyanın eksik bilgileri: en az '
                    + sayi(t.istek) + ' istek, ' + sureYazisi(t.sure_sn) + '.',
                e('button', { sinif: 'i-d' + (yakin ? '' : ' ana'), type: 'button', metin: 'Evet, listeyi de al', onclick: function() { dlBaslat('hepsi'); } })),
            secenek('Hayır, yalnız eksikleri al',
                eksikYok ? 'Süzgeçteki açık dosyaların hiçbirinde eksik yok; UYAP\'a istek gerekmiyor.'
                    : 'Liste yeniden alınmaz. Süzgeçteki ' + sayi(x.hedef) + ' açık dosyanın eksik taraf ve takip türü bilgileri, listedeki sırayla: ≈ '
                        + sayi(x.istek) + ' istek, ' + sureYazisi(x.sure_sn) + '.',
                e('button', { sinif: 'i-d' + (yakin && !eksikYok ? ' ana' : ''), type: 'button', metin: 'Hayır, yalnız eksikler', disabled: eksikYok,
                    onclick: function() { dlBaslat('eksik'); } })),
            mesaiMi() ? e('div', { sinif: 'i-gt-uyari', role: 'note',
                metin: 'Mesai saatindesiniz: istekler büronun UYAP sorgularıyla çakışabilir. Akşam 18:00\'den sonra çalıştırmanız önerilir.' }) : null,
            e('div', { sinif: 'i-eylem' }, [e('button', { sinif: 'i-d', type: 'button', metin: 'Vazgeç', onclick: function() { d.dl.onay = null; dlCiz(); } })])
        ]);
    }
    // Onay kutusu: ne yapılacağı, kaç dosya / istek, tahmini süre; mesai saatindeyse uyarı.
    function dlOnayKutusu(o) {
        if (o.mod === 'hepsi' && o.eksik) return dlHepsiOnayi(o);
        const t = o.t;
        const satirlar = [];
        let bos_ = false;
        if (o.mod === 'liste') {
            satirlar.push(listeZamaniBilgisi(t.liste_zamani).metin);
            satirlar.push('UYAP\'taki açık dosya listesi alınacak (süzgeç uygulanmaz): en az ' + sayi(t.istek) + ' istek, ' + sureYazisi(t.sure_sn) + '.');
            satirlar.push('Kapanan dosyalar ve yeni gelen dosyaların takip türü için dosya başına ek istek gider.');
        } else if (o.mod === 'hepsi') {
            satirlar.push('Dosya listesi alınacak, ardından süzgeçteki ' + sayi(t.dosya) + ' dosyadan ' + sayi(t.hedef) + ' açık dosyanın eksik bilgileri: en az '
                + sayi(t.istek) + ' istek, ' + sureYazisi(t.sure_sn) + '.');
            satirlar.push('Yeni ve kapanan dosyalar için ek istek gider.');
        } else if (!t.hedef) {
            bos_ = true;
            satirlar.push('Süzgeçteki ' + sayi(t.dosya) + ' dosyanın hiçbirinde eksik yok; UYAP\'a istek gerekmiyor.');
        } else {
            satirlar.push('Süzgeçteki ' + sayi(t.dosya) + ' dosyadan ' + sayi(t.hedef) + ' dosyaya istek gidecek: ≈ ' + sayi(t.istek) + ' istek, ' + sureYazisi(t.sure_sn) + '.');
            satirlar.push('Diğer ' + sayi(t.dosya - t.hedef) + ' dosyada bu bilgi zaten tam; onlara istek gitmez.');
        }
        const ayrinti = [];
        if (o.mod !== 'liste' && t.hedef) {
            if (o.mod !== 'taraf' || t.takip) ayrinti.push('takip türü okunmamış: ' + sayi(t.takip));
            if (o.mod !== 'takip') ayrinti.push('tarafı eksik: ' + sayi(t.taraf_tum), 'borçlu kimliği eksik: ' + sayi(t.taraf_kimliksiz),
                'vekilleri okunmamış: ' + sayi(t.vekil_okunmamis || 0));
        }
        const kutu = e('div', { sinif: 'i-gt', role: 'region', 'aria-label': o.ad }, [
            e('div', { stil: 'font-weight:600;margin-bottom:6px', metin: o.ad }),
            e('div', {}, satirlar.map(function(m) { return e('div', { sinif: 'i-gt-not', stil: 'color:var(--i-yazi)', metin: m }); })),
            ayrinti.length ? e('div', { sinif: 'i-gt-not', stil: 'margin-top:4px', metin: 'Ayrıntı: ' + ayrinti.join(' · ') }) : null,
            !bos_ && mesaiMi() ? e('div', { sinif: 'i-gt-uyari', role: 'note',
                metin: 'Mesai saatindesiniz: istekler büronun UYAP sorgularıyla çakışabilir. Akşam 18:00\'den sonra çalıştırmanız önerilir.' }) : null,
            e('div', { sinif: 'i-eylem' }, bos_ ? [
                e('button', { sinif: 'i-d', type: 'button', metin: 'Kapat', onclick: function() { d.dl.onay = null; dlCiz(); } })
            ] : [
                e('button', { sinif: 'i-d ana', type: 'button', metin: 'Başlat', onclick: function() { dlBaslat(o.mod); } }),
                e('button', { sinif: 'i-d', type: 'button', metin: 'Vazgeç', onclick: function() { d.dl.onay = null; dlCiz(); } })
            ])
        ]);
        return kutu;
    }

    function dlCiz() {
        const odakVardi = !!(document.activeElement && document.activeElement.closest && document.activeElement.closest('.i-dlp'));
        // Aynı liste yeniden çiziliyorsa (dosya paneli, onay kutusu, tarama durumu) kaydırma yeri korunur; süzgeç, sıra
        // ya da sayfa değiştiyse baştan başlar.
        const eskiAna = govde.querySelector('.i-ana');
        const imza = dlSorgusu();
        const yer = eskiAna && d.dl.cizimImzasi === imza ? eskiAna.scrollTop : 0;
        d.dl.cizimImzasi = imza;
        bos(govde);
        dlAcilirKaldir();
        const ana = e('div', { sinif: 'i-ana' });
        govde.appendChild(ana);
        const f = d.dl.filtre, l = d.dl.liste;
        const yenile = function() { f.sayfa = 1; yukle(); };

        // Başlık satırı: dosya sayısı + güncelleme düğmeleri (toplu tarama paneli bunun altına yerleşir).
        const suruyor = gtSuruyor();
        const dugme = function(mod, title, birincil) {
            return e('button', { sinif: 'i-d m-ek' + (birincil ? ' ana' : ''), type: 'button', metin: DL_TURLER[mod], title: title,
                disabled: suruyor, 'aria-pressed': d.dl.onay && d.dl.onay.mod === mod ? 'true' : null, onclick: function() { dlGuncelle(mod); } });
        };
        ana.appendChild(e('div', { sinif: 'i-arac' }, [
            e('div', { sinif: 'i-dl-sayi', role: 'status' }, l ? [e('b', { metin: sayi(l.toplam) }), ' dosya listeleniyor',
                l.genel_toplam != null && l.genel_toplam !== l.toplam ? e('span', { sinif: 'alt', metin: ' · veritabanında toplam ' + sayi(l.genel_toplam) + ' icra dosyası' }) : null] : []),
            e('div', { sinif: 'i-bosluk' }), mAracDugmesi(ana),
            dugme('hepsi', 'Dosya listesini alır (yeni ve kapanan dosyalar dahil), ardından süzgeçteki açık dosyaların eksik taraflarını ve okunmamış takip türlerini UYAP\'tan alır.', true),
            dugme('liste', 'UYAP\'taki açık icra dosyalarınızın listesini alır: yeni dosyalar eklenir, kapananlar işaretlenir. Süzgeç uygulanmaz.'),
            dugme('takip', 'Süzgeçteki dosyalardan takip türü, yolu ve şekli henüz okunmamış olanları UYAP\'tan okur (dosya başına 1 istek).'),
            dugme('taraf', 'Süzgeçteki dosyalardan tarafı ya da borçlu kimliği eksik olanların alacaklı, borçlu ve vekil bilgilerini UYAP\'tan alır.')
        ]));
        if (d.dl.onay) ana.appendChild(dlOnayKutusu(d.dl.onay));
        if (gtGorunsun()) ana.appendChild(gtPaneli());

        // Durum ve tarih aralıkları
        const tarih = function(k, ad) {
            const t = e('input', { type: 'date', value: f[k], title: ad, 'aria-label': ad });
            t.addEventListener('change', function() { f[k] = t.value; yenile(); });
            return t;
        };
        const alan = function(ad, cocuklar) { return e('div', { sinif: 'i-dl-alan' }, [e('span', { metin: ad })].concat(cocuklar)); };
        const sira = dlSiraEsitle();
        const tarihSuzgecleri = dlTarihSuzgecleri();
        const suzgecSayisi = sira.length + tarihSuzgecleri.length;
        const herhangi = suzgecSayisi > 0;
        const temizle = dlTumunuTemizle;
        // Yıl / ay düğmesi: seçim varsa özetini gösterir ("2025, 2024 · Mart").
        const yilAyDugmesi = function(onEk) {
            const cy = DL_YILAY[onEk + '_yil'], ca = DL_YILAY[onEk + '_ay'];
            const ozet = [f[cy.k] ? dlSuzgecOzeti(cy) : '', f[ca.k] ? dlSuzgecOzeti(ca) : ''].filter(Boolean).join(' · ');
            const id = onEk + '_ya';
            return e('button', { type: 'button', sinif: 'i-d i-dl-yilay' + (ozet ? ' dolu' : ''), 'data-dl': id, 'aria-haspopup': 'dialog',
                'aria-expanded': d.dl.acik === id ? 'true' : 'false', title: 'Yıla ve aya göre süz (birden çok yıl / ay seçilebilir; yalnız ay da olur)',
                onclick: function() { dlAc(id); } }, [e('span', { metin: ozet || 'Yıl / ay' }), huni()]);
        };
        ana.appendChild(e('div', { sinif: 'i-dl-suzgec m-ek', role: 'group', 'aria-label': 'Süzgeçler' }, [
            alan('Dosya durumu', [e('div', { sinif: 'i-seg' }, DL_DURUMLAR.map(function(x) {
                return e('button', { type: 'button', sinif: f.durum === x[0] ? 'on' : '', 'aria-pressed': f.durum === x[0] ? 'true' : 'false', metin: x[1],
                    onclick: function() { f.durum = x[0]; yenile(); } });
            }))]),
            alan('Açılış tarihi', [e('div', { sinif: 'i-dl-tarih' }, [tarih('acilis_bas', 'Açılış tarihi: başlangıç'), '–', tarih('acilis_son', 'Açılış tarihi: bitiş'),
                yilAyDugmesi('acilis')])]),
            f.durum === 'kapali' ? alan('Kapanış tarihi', [e('div', { sinif: 'i-dl-tarih' },
                [tarih('kapanis_bas', 'Kapanış tarihi: başlangıç'), '–', tarih('kapanis_son', 'Kapanış tarihi: bitiş'), yilAyDugmesi('kapanis')])]) : null,
            e('div', { sinif: 'i-dl-suzgec-sag' }, [
                e('button', { type: 'button', sinif: 'i-d' + (herhangi ? ' dolu' : ''), 'data-dl': 'suzgecler', 'aria-haspopup': 'dialog',
                    'aria-expanded': d.dl.acik === 'suzgecler' ? 'true' : 'false',
                    title: 'Konan süzgeçler: kaldırın ya da sürükleyerek sırasını değiştirin (üstteki alttakini kapsar)',
                    onclick: function() { dlAc('suzgecler'); } }, [huni(), e('span', { metin: 'Süzgeçler' + (suzgecSayisi ? ' (' + suzgecSayisi + ')' : '') })]),
                herhangi ? e('button', { sinif: 'i-dahasi', type: 'button', stil: 'padding:6px 0', metin: 'Tüm süzgeçleri temizle', onclick: temizle }) : null
            ])
        ]));
        // Konan süzgeçler (Süreçler sekmesindeki gibi): sırayla etiketler; ✕ ile kaldırılır, etikete basınca Süzgeçler açılır.
        if (herhangi) {
            const etiketler = tarihSuzgecleri.map(function(t) { return { ad: t.ad, metin: t.metin, kaldir: function() { t.kaldir(); f.sayfa = 1; yukle(); } }; })
                .concat(sira.map(function(k) {
                    const c = DL_SUZGECLER.find(function(x) { return x.k === k; });
                    return { ad: c.ad, metin: dlSuzgecOzeti(c), kaldir: function() { dlSuzgecKaldir(k); } };
                }));
            ana.appendChild(e('div', { sinif: 'i-secili i-dl-secili' }, [e('span', { metin: 'Süzgeç:' })].concat(etiketler.map(function(x, i) {
                return e('span', { sinif: 'et' }, [
                    e('button', { type: 'button', sinif: 'et-ad', title: 'Süzgeçleri göster (sırası ve kaldırma)', onclick: function() { dlAc('suzgecler'); } },
                        [e('span', { metin: x.ad + ':' }), e('b', { metin: x.metin })]),
                    e('button', { type: 'button', sinif: 'et-x', metin: '✕', title: x.ad + ' süzgecini kaldır', 'aria-label': x.ad + ' süzgecini kaldır', onclick: x.kaldir })
                ]);
            }))));
        }
        ana.appendChild(e('div', { sinif: 'i-dl-ipucu m-ek' }, [
            e('span', { metin: 'Süzmek ya da sıralamak için sütun başlığına tıklayın.' }),
            l && l.avukat_bilinmiyor ? e('span', { metin: 'Rolüm: e-imza avukatının adı, program UYAP\'a e-imzayla bir sonraki girişte kayda geçer.' }) : null
        ]));

        // Tablo
        const sutunlar = DL_SUTUN.filter(function(c) { return !(c.kapali && f.durum === 'acik'); });
        const basliklar = sutunlar.map(function(c) {
            const id = c.k || c.s;
            // Sütunun süzgeci (Dosya No'da yıl; tarih sütunlarında aralık ve yıl / ay da sayılır).
            const dolu = !!(c.k && f[c.k]) || (id === 'dosya_no' && !!f.yil)
                || (id === 'acilis' && !!(f.acilis_bas || f.acilis_son || f.acilis_yil || f.acilis_ay))
                || (id === 'kapanis' && !!(f.kapanis_bas || f.kapanis_son || f.kapanis_yil || f.kapanis_ay));
            const ok = f.sirala === c.s ? (f.yon === 'azalan' ? '↓' : '↑') : '';
            return e('th', {}, [e('button', { type: 'button', sinif: dolu ? 'dolu' : '', 'data-dl': id,
                'aria-expanded': d.dl.acik === id ? 'true' : 'false', 'aria-haspopup': 'dialog', title: c.ad + ': süzmek ya da sıralamak için tıklayın',
                onclick: function() { dlAc(c); } },
                [e('span', { metin: c.ad }), ok ? e('span', { sinif: 'ok', metin: ok }) : null,
                 dolu ? e('span', { sinif: 'nokta', title: 'Süzgeç var' }) : null, huni()])]);
        });
        const tablo = e('table', { sinif: 'dl' }, [e('thead', {}, [e('tr', {}, basliklar)])]);
        const silik = function(m, t) { return e('span', { sinif: 'silik', metin: m, title: t }); };
        const coklu = function(liste) {
            if (!liste || !liste.length) return [silik('—')];
            return [liste[0], liste.length > 1 ? e('span', { sinif: 'fazla', metin: '+' + (liste.length - 1) }) : null];
        };
        const hucre = function(c, s) {
            switch (c.k || c.s) {
                case 'birim': return e('td', { sinif: 'tek', title: s.birim, metin: s.birim });
                case 'dosya_no': return e('td', { sinif: 'say' }, [e('span', { sinif: 'dno', metin: s.dosya_no })]);
                case 'dosya_tur': return e('td', { sinif: 'tek', title: s.mts_kaynakli ? s.dosya_turu + ': Merkezi Takip Sistemi\'nden icra dairesine geçmiş dosya' : s.dosya_turu },
                    [s.dosya_turu || silik('—')]);
                case 'alacakli': return e('td', { sinif: 'tek', title: s.alacaklilar.join('\n') }, coklu(s.alacaklilar));
                case 'borclu': return e('td', { sinif: 'tek', title: s.borclular.join('\n') }, coklu(s.borclular));
                case 'rol': {
                    // Rol türü + vekili olunan taraf(lar)ın adı (kullanıcı isteği 20.11.2026).
                    if (!s.rol) return e('td', {}, [silik('—', l.avukat_bilinmiyor ? 'E-imza sahibi avukatın adı henüz tanımlı değil' : 'Avukatın adı bu dosyanın vekil listelerinde yok')]);
                    const tr = s.rol_taraflar || [];
                    return e('td', { sinif: 'rol', title: s.rol_etiket + (tr.length ? ': ' + tr.join(', ') : '') }, [
                        e('span', { sinif: 'chip', stil: '--r:' + (DL_ROL_RENK[s.rol] || '#8a877e'), metin: s.rol_etiket }),
                        tr.length ? e('div', { sinif: 'rol-taraf' }, [tr[0], tr.length > 1 ? e('span', { sinif: 'fazla', metin: '+' + (tr.length - 1) }) : null]) : null
                    ]);
                }
                case 'takip_turu': return e('td', {}, [s.takip_turu || silik('—', 'Takip türü henüz okunmadı')]);
                case 'takip_yolu': return e('td', { sinif: 'tek', title: s.takip_yolu }, [s.takip_yolu || silik('—', 'Takip yolu henüz okunmadı')]);
                case 'takip_sekli': return e('td', { sinif: 'tek', title: s.takip_sekli }, [s.takip_sekli || silik('—', 'Takip şekli henüz okunmadı')]);
                case 'acilis': return e('td', { sinif: 'say' }, [s.acilis_tarihi ? gunMetni(s.acilis_tarihi) : silik('—')]);
                case 'kapanis': return e('td', { sinif: 'say', title: s.kapanis_sebebi || s.durum || '' },
                    [s.acik ? silik('—') : s.kapanis_tarihi ? gunMetni(s.kapanis_tarihi) : silik(s.durum || 'Kapalı', 'Durum metninde kapanış günü yok')]);
            }
            return e('td', {});
        };
        if (!l || !l.satirlar.length) {
            tablo.appendChild(e('tbody', {}, [e('tr', {}, [e('td', { colspan: String(sutunlar.length), sinif: 'silik', stil: 'text-align:center;padding:26px' },
                [l ? 'Bu ölçütlere uyan dosya yok. ' : 'Yükleniyor…',
                 l && herhangi ? e('button', { sinif: 'i-dahasi', type: 'button', metin: 'Süzgeçleri temizle', onclick: temizle }) : null])])]));
        } else {
            l.satirlar.forEach(function(s) {
                tablo.appendChild(e('tbody', { sinif: 'tik' + (d.secili && d.secili.id === s.dosya ? ' sec' : ''), onclick: function() { dosyaAc(s.dosya); } },
                    [hucreEtiketle(e('tr', {}, sutunlar.map(function(c) { return hucre(c, s); })), sutunlar)]));
            });
        }
        ana.appendChild(e('div', { sinif: 'i-dl-kutu' }, [tablo]));
        if (l && l.toplam > l.boyut) {
            const son = Math.max(1, Math.ceil(l.toplam / l.boyut));
            const git = function(n) { return function() { f.sayfa = n; yukle(); }; };
            ana.appendChild(e('div', { sinif: 'i-sayfa' }, [
                e('span', { metin: ((l.sayfa - 1) * l.boyut + 1) + '–' + Math.min(l.toplam, l.sayfa * l.boyut) + ' / ' + l.toplam + ' dosya gösteriliyor' }),
                e('span', {}, [
                    e('button', { sinif: 'i-d kucuk', metin: '‹ Önceki', disabled: l.sayfa <= 1, onclick: git(l.sayfa - 1) }), ' ', l.sayfa + ' / ' + son, ' ',
                    e('button', { sinif: 'i-d kucuk', metin: 'Sonraki ›', disabled: l.sayfa >= son, onclick: git(l.sayfa + 1) })
                ])
            ]));
        }
        if (d.secili) panelCiz();
        // Panel eklendikten sonra (genişlik değişir) eski yere dön; bu kaydırma süzgeç penceresini kapatmasın.
        let kendiKaydirma = false;
        if (yer > 0) { kendiKaydirma = true; ana.scrollTop = yer; setTimeout(function() { kendiKaydirma = false; }, 200); }
        ana.addEventListener('scroll', function() { if (kendiKaydirma) { kendiKaydirma = false; return; } if (d.dl.acik) dlKapat(); });
        if (d.dl.acik) dlAcilirCiz(odakVardi);
    }

    // ── Kesinleştirme (22.11.2026; kullanıcı notu Desktop\süreçler\kesinleştirme sekmesi.md) ──
    // Borçlu başına satır (src/icra_kesinlestirme.rs): yalnız alacaklı vekili olduğumuz "İcra Dosyası" türündeki açık
    // dosyalar (talimat, iflas, MTS ve ihtiyati haciz hariç). Üstte durum grupları (adetli; basınca süzer). Süzgeç
    // pencereleri Dosya Listesi ile ortak (D()). Düğmeler: "Kesinleşmeyi UYAP'tan oku" (dosya başına 1 istek; sorgu
    // yapmadan, UYAP'ın kesinleşen borçlu listesinden) ve "Kesinleştirme talebi gönder" (Takibin Kesinleşmesi genel
    // talebi, e-imzalı; önce onay kutusu). Daire kendiliğinden kesinleştirebildiği için her talepten önce UYAP'a sorulur.
    const KS_SUTUN = [
        { ad: 'İcra Dairesi', k: 'birim', s: 'birim' },
        { ad: 'Dosya No', k: 'dosya_no', s: 'dosya_no' },
        { ad: 'Alacaklı', k: 'alacakli', s: 'alacakli' },
        { ad: 'Borçlu', k: 'borclu', s: 'borclu' },
        { ad: 'Takip Yolu', k: 'takip_yolu', s: 'takip_yolu' },
        { ad: 'Aşama', k: 'asama', s: 'asama', kodlu: true },
        { ad: 'Ödeme Emri Tebligatı', k: 'mazbata', s: 'mazbata', kodlu: true },
        { ad: 'Kesinleşme', s: 'kesinlesme', tarih: true,
          not: 'Programın hesapladığı kesinleşme gününe göre sıralayın (tebliğ tarihi + takip türünün kesinleşme süresi; mazbata dönmeden hesaplanmaz).' },
        { ad: 'Durum', k: 'grup', s: 'grup', kodlu: true },
        { ad: 'UYAP', k: 'uyap', s: 'uyap', kodlu: true },
        { ad: 'Talep', k: 'talep', s: 'talep', kodlu: true },
        { ad: 'Yeniden Tebliğ', k: 'yeniden', s: 'yeniden', kodlu: true,
          not: 'Ödeme emri iade dönen borçlu: Mernis adresi ilk giden tebligatın adresiyle aynıysa 21/2 şerhli, farklıysa Mernis adresine normal tebligat.' },
        { ad: 'Açılış', s: 'acilis', tarih: true }
    ];
    const KS_SUZGECLER = KS_SUTUN.filter(function(c) { return c.k; }).concat([DL_YIL, DL_YILAY.acilis_yil, DL_YILAY.acilis_ay]);
    function ksFiltreBos() {
        const f = { acilis_bas: '', acilis_son: '', sirala: '', yon: 'artan', sayfa: 1, sira: [] };
        KS_SUZGECLER.forEach(function(c) { f[c.k] = ''; });
        return f;
    }
    d.ks = { filtre: ksFiltreBos(), liste: null, acik: null, yazi: '', onay: null, cizimImzasi: '',
             sutun: KS_SUTUN, suzgecler: KS_SUZGECLER, bos: ksFiltreBos };
    const KS_GRUP_RENK = { kesinlestirilebilir: 'var(--i-vurgu)', talep_sonucsuz: '#b84444', talep_gonderildi: '#3f7fa3',
                           sure_isliyor: '#b8901f', bekleniyor: '#8a877e', uyap_kesinlesti: '#4f8a57', durdu: '#7a5fb0' };
    const KS_ISLER = { talep: 'Kesinleştirme talebi gönder', kesinlesme: 'Kesinleşmeyi UYAP\'tan oku',
                       mernis: 'Mernis adresini sorgula', teblig: 'Yeniden tebliğ talebi gönder' };
    const KS_MOD = { talep: 'kesinlestirme_talebi', kesinlesme: 'kesinlesme', mernis: 'mernis', teblig: 'yeniden_teblig' };

    // Başlatmadan önce tahmin (kaç dosya / borçlu, kaç istek, ne kadar sürer); kullanıcı onaylayınca gider.
    async function ksGuncelle(is) {
        if (gtSuruyor()) { d.gtAcik = true; gtCiz(); return; }
        try { d.ks.onay = { is: is, t: await istek('kesinlestirme_istek?' + dlSorgusu(true) + '&is=' + is) }; }
        catch (x) { bildir(x.message); return; }
        ksCiz();
    }
    async function ksBaslat(is) {
        d.ks.onay = null;
        try {
            const k = await istek('kesinlestirme_kimlikleri?' + dlSorgusu(true) + '&is=' + is);
            if (!k.dosyalar.length) { bildir('Süzgeçte bu işe uygun borçlu yok.'); ksCiz(); return; }
            const govde = { mod: KS_MOD[is], dosyalar: k.dosyalar };
            if (is !== 'kesinlesme') govde.borclular = k.borclular;
            d.gt = await gtIstek('', govde);
            d.gtAcik = true;
            gtYokla();
        } catch (x) { bildir(x.message); }
        if (d.sekme === 'kesin') ksCiz();
    }
    function ksOnayKutusu(o) {
        if (o.is === 'mernis' || o.is === 'teblig') return ksMernisOnayi(o);
        const t = o.t, talep = o.is === 'talep';
        const satirlar = [];
        const bos_ = talep ? !t.borclu : !t.dosya;
        if (bos_) {
            satirlar.push(talep ? 'Süzgeçteki ' + sayi(t.satir) + ' borçlunun hiçbiri kesinleştirilebilir değil (ya da talebi zaten gönderildi); talep gönderilmeyecek.'
                                : 'Süzgeçteki borçluların hepsi UYAP\'ta kesinleşmiş ya da itiraz / durdurma var; UYAP\'a sorulacak dosya yok.');
        } else if (talep) {
            satirlar.push('Süzgeçteki ' + sayi(t.borclu) + ' borçluya (' + sayi(t.dosya) + ' dosya) "Takibin Kesinleşmesi" talebi gönderilecek: ≈ '
                + sayi(t.istek) + ' istek, ' + sureYazisi(t.sure_sn) + '.');
            satirlar.push('Her talep UYAP\'ta hazırlanır, takılı e-imza kartıyla imzalanır ve gönderilir; gönderilen talep geri alınamaz.');
            satirlar.push('Gönderimden hemen önce borçlunun UYAP\'ta zaten kesinleşmiş olup olmadığına bakılır; kesinleşmişse talep gönderilmez. Bugün talep gönderilmiş borçluya yeniden gönderilmez.');
        } else {
            satirlar.push('Süzgeçteki ' + sayi(t.borclu) + ' borçlunun ' + sayi(t.dosya) + ' dosyasında kesinleşme UYAP\'tan okunacak (sorgu yapılmaz): ≈ '
                + sayi(t.istek) + ' istek, ' + sureYazisi(t.sure_sn) + '.');
            satirlar.push('UYAP\'ta kesinleşmiş görünen borçluların İş Listesi\'ndeki kesinleştirme işleri otomatik tamamlanır.');
        }
        return e('div', { sinif: 'i-gt', role: 'region', 'aria-label': KS_ISLER[o.is] }, [
            e('div', { stil: 'font-weight:600;margin-bottom:6px', metin: KS_ISLER[o.is] }),
            e('div', {}, satirlar.map(function(m) { return e('div', { sinif: 'i-gt-not', stil: 'color:var(--i-yazi)', metin: m }); })),
            !bos_ && mesaiMi() ? e('div', { sinif: 'i-gt-uyari', role: 'note',
                metin: 'Mesai saatindesiniz: istekler büronun UYAP sorgularıyla çakışabilir. Akşam 18:00\'den sonra çalıştırmanız önerilir.' }) : null,
            e('div', { sinif: 'i-eylem' }, bos_ ? [
                e('button', { sinif: 'i-d', type: 'button', metin: 'Kapat', onclick: function() { d.ks.onay = null; ksCiz(); } })
            ] : [
                e('button', { sinif: 'i-d ana', type: 'button', metin: talep ? 'E-imzala ve gönder (' + sayi(t.borclu) + ' borçlu)' : 'Başlat',
                    onclick: function() { ksBaslat(o.is); } }),
                e('button', { sinif: 'i-d', type: 'button', metin: 'Vazgeç', onclick: function() { d.ks.onay = null; ksCiz(); } })
            ])
        ]);
    }

    // Mernis sorgusu (salt okuma) ve yeniden tebliğ talebi (ÜCRETLİ: borçlu başına ≈ 265 TL, e-Barobirlik kartından).
    function ksMernisOnayi(o) {
        const t = o.t, teblig = o.is === 'teblig';
        const satirlar = [];
        const bos_ = !t.borclu;
        if (bos_) {
            satirlar.push(teblig
                ? 'Süzgeçte yeniden tebliğ talebine hazır borçlu yok: önce "Mernis adresini sorgula" ile ödeme emri iade dönen borçluların Mernis adresi alınmalı (elle incelenmesi gerekenler ve talebi gönderilmiş olanlar dahil edilmez).'
                : 'Süzgeçte ödeme emri iade dönmüş (ve yeniden tebliğ talebi gönderilmemiş) borçlu yok; UYAP\'a sorgu atılmayacak.');
        } else if (teblig) {
            satirlar.push('Süzgeçteki ' + sayi(t.borclu) + ' borçluya (' + sayi(t.dosya) + ' dosya) yeniden tebliğ talebi gönderilecek: ' +
                sayi(t.uygun) + ' borçluya T.K. 21/2 şerhli, ' + sayi(t.farkli) + ' borçluya Mernis adresine normal tebligat.');
            satirlar.push('ÜCRETLİDİR: toplam ≈ ' + sayi(t.tutar) + ' TL e-Barobirlik kartından düşer. Gönderilen talep geri alınamaz.');
            satirlar.push('Her borçlu için göndermeden hemen önce dosyanın evraklarına bakılır; iade sonrası tebligat talebi evrakı varsa gönderilmez. ≈ ' +
                sayi(t.istek) + ' istek, ' + sureYazisi(t.sure_sn) + '.');
        } else {
            satirlar.push('Süzgeçte ödeme emri iade dönmüş ' + sayi(t.borclu) + ' borçlunun (' + sayi(t.dosya) + ' dosya) Mernis adresi UYAP\'tan sorgulanacak ve borçluya giden ilk ödeme emri zarfının adresiyle karşılaştırılacak: ≈ ' +
                sayi(t.istek) + ' istek, ' + sureYazisi(t.sure_sn) + '.');
            satirlar.push('Aynı kişi son 30 günde başka bir dosyada sorgulandıysa UYAP\'a yeniden sorulmaz. Sonuç borçlunun dosyadaki kaydına yazılır; talep gönderilmez.');
        }
        if (!o.anladim) o.anladim = false;
        const onay = teblig && !bos_ ? e('label', { sinif: 'i-gt-uyari', stil: 'display:flex;gap:6px;align-items:flex-start;cursor:pointer' }, [
            e('input', { type: 'checkbox', checked: o.anladim, 'aria-label': 'Ücretli olduğunu anladım',
                onchange: function(ev) { o.anladim = ev.target.checked; ksCiz(); } }),
            'Talebin ücretli olduğunu ve geri alınamayacağını anladım.']) : null;
        return e('div', { sinif: 'i-gt', role: 'region', 'aria-label': KS_ISLER[o.is] }, [
            e('div', { stil: 'font-weight:600;margin-bottom:6px', metin: KS_ISLER[o.is] }),
            e('div', {}, satirlar.map(function(m) { return e('div', { sinif: 'i-gt-not', stil: 'color:var(--i-yazi)', metin: m }); })),
            !bos_ && mesaiMi() ? e('div', { sinif: 'i-gt-uyari', role: 'note',
                metin: 'Mesai saatindesiniz: istekler büronun UYAP sorgularıyla çakışabilir. Akşam 18:00\'den sonra çalıştırmanız önerilir.' }) : null,
            onay,
            e('div', { sinif: 'i-eylem' }, bos_ ? [
                e('button', { sinif: 'i-d', type: 'button', metin: 'Kapat', onclick: function() { d.ks.onay = null; ksCiz(); } })
            ] : [
                e('button', { sinif: 'i-d ana', type: 'button', disabled: teblig && !o.anladim,
                    metin: teblig ? 'E-imzala ve gönder (' + sayi(t.borclu) + ' borçlu, ≈ ' + sayi(t.tutar) + ' TL)' : 'Başlat',
                    onclick: function() { ksBaslat(o.is); } }),
                e('button', { sinif: 'i-d', type: 'button', metin: 'Vazgeç', onclick: function() { d.ks.onay = null; ksCiz(); } })
            ])
        ]);
    }

    function ksCiz() {
        const odakVardi = !!(document.activeElement && document.activeElement.closest && document.activeElement.closest('.i-dlp'));
        const eskiAna = govde.querySelector('.i-ana');
        const imza = dlSorgusu();
        const yer = eskiAna && d.ks.cizimImzasi === imza ? eskiAna.scrollTop : 0;
        d.ks.cizimImzasi = imza;
        bos(govde);
        dlAcilirKaldir();
        const ana = e('div', { sinif: 'i-ana' });
        govde.appendChild(ana);
        const f = d.ks.filtre, l = d.ks.liste;
        const suruyor = gtSuruyor();
        const dugme = function(is, title, birincil) {
            return e('button', { sinif: 'i-d m-ek' + (birincil ? ' ana' : ''), type: 'button', metin: KS_ISLER[is], title: title, disabled: suruyor,
                'aria-pressed': d.ks.onay && d.ks.onay.is === is ? 'true' : null, onclick: function() { ksGuncelle(is); } });
        };
        ana.appendChild(e('div', { sinif: 'i-arac' }, [
            e('div', { sinif: 'i-dl-sayi', role: 'status' }, l ? [e('b', { metin: sayi(l.toplam) }), ' borçlu listeleniyor',
                l.genel_toplam !== l.toplam ? e('span', { sinif: 'alt', metin: ' · kapsamda toplam ' + sayi(l.genel_toplam) + ' borçlu' }) : null] : []),
            e('div', { sinif: 'i-bosluk' }), mAracDugmesi(ana),
            dugme('talep', 'Süzgeçteki kesinleştirilebilir borçlulara "Takibin Kesinleşmesi" talebini e-imzayla gönderir (önce onay sorulur).', true),
            dugme('kesinlesme', 'Süzgeçteki dosyalarda takibin UYAP\'ta kesinleşip kesinleşmediğini okur (dosya başına 1 istek; sorgu yapılmaz).'),
            dugme('mernis', 'Ödeme emri iade dönen borçluların Mernis adresini sorgular ve ilk giden tebligatın adresiyle karşılaştırır (talep gönderilmez).'),
            dugme('teblig', 'Mernis kıyası yapılmış borçlulara yeniden tebliğ talebi: adres aynıysa 21/2 şerhli, farklıysa Mernis adresine normal tebligat (ücretli; önce onay sorulur).')
        ]));
        if (d.ks.onay) ana.appendChild(ksOnayKutusu(d.ks.onay));
        if (gtGorunsun()) ana.appendChild(gtPaneli());

        // Durum grupları: sayılar öteki süzgeçlere uyan borçlulardan; basınca yalnız o grup (yeniden basınca kaldırılır).
        const grupAdet = {};
        ((l && l.secenekler && l.secenekler.grup) || []).forEach(function(o) { grupAdet[o.deger] = o.adet; });
        const secGrup = f.grup || '';
        ana.appendChild(e('div', { sinif: 'i-asamalar', role: 'group', 'aria-label': 'Durum grupları' }, ((l && l.gruplar) || []).map(function(g) {
            const on = secGrup === g.kod;
            return e('button', { sinif: 'i-asama' + (on ? ' on' : ''), type: 'button', title: g.etiket, 'aria-pressed': on ? 'true' : 'false',
                onclick: function() { dlUygula('grup', on ? '' : g.kod); } },
                [e('div', { sinif: 'n', metin: sayi(grupAdet[g.kod] || 0) }), e('div', { sinif: 't', metin: g.etiket }),
                 e('i', { stil: 'background:' + (KS_GRUP_RENK[g.kod] || '#8a877e') })]);
        })));

        // Açılış tarihi, Süzgeçler penceresi
        const tarih = function(k, ad) {
            const t = e('input', { type: 'date', value: f[k], title: ad, 'aria-label': ad });
            t.addEventListener('change', function() { f[k] = t.value; f.sayfa = 1; yukle(); });
            return t;
        };
        const sira = dlSiraEsitle();
        const tarihSuzgecleri = dlTarihSuzgecleri();
        const suzgecSayisi = sira.length + tarihSuzgecleri.length;
        const herhangi = suzgecSayisi > 0;
        const ya = DL_YILAY.acilis_yil, ay = DL_YILAY.acilis_ay;
        const yaOzet = [f[ya.k] ? dlSuzgecOzeti(ya) : '', f[ay.k] ? dlSuzgecOzeti(ay) : ''].filter(Boolean).join(' · ');
        ana.appendChild(e('div', { sinif: 'i-dl-suzgec m-ek', role: 'group', 'aria-label': 'Süzgeçler' }, [
            e('div', { sinif: 'i-dl-alan' }, [e('span', { metin: 'Açılış tarihi' }), e('div', { sinif: 'i-dl-tarih' }, [
                tarih('acilis_bas', 'Açılış tarihi: başlangıç'), '–', tarih('acilis_son', 'Açılış tarihi: bitiş'),
                e('button', { type: 'button', sinif: 'i-d i-dl-yilay' + (yaOzet ? ' dolu' : ''), 'data-dl': 'acilis_ya', 'aria-haspopup': 'dialog',
                    'aria-expanded': d.ks.acik === 'acilis_ya' ? 'true' : 'false', title: 'Açılış yılına ve ayına göre süz',
                    onclick: function() { dlAc('acilis_ya'); } }, [e('span', { metin: yaOzet || 'Yıl / ay' }), huni()])])]),
            e('div', { sinif: 'i-dl-suzgec-sag' }, [
                e('button', { type: 'button', sinif: 'i-d' + (herhangi ? ' dolu' : ''), 'data-dl': 'suzgecler', 'aria-haspopup': 'dialog',
                    'aria-expanded': d.ks.acik === 'suzgecler' ? 'true' : 'false', title: 'Konan süzgeçler: kaldırın ya da sürükleyerek sırasını değiştirin',
                    onclick: function() { dlAc('suzgecler'); } }, [huni(), e('span', { metin: 'Süzgeçler' + (suzgecSayisi ? ' (' + suzgecSayisi + ')' : '') })]),
                herhangi ? e('button', { sinif: 'i-dahasi', type: 'button', stil: 'padding:6px 0', metin: 'Tüm süzgeçleri temizle', onclick: dlTumunuTemizle }) : null
            ])
        ]));
        if (herhangi) {
            const etiketler = tarihSuzgecleri.map(function(t) { return { ad: t.ad, metin: t.metin, kaldir: function() { t.kaldir(); f.sayfa = 1; yukle(); } }; })
                .concat(sira.map(function(k) {
                    const c = KS_SUZGECLER.find(function(x) { return x.k === k; });
                    return { ad: c.ad, metin: dlSuzgecOzeti(c), kaldir: function() { dlSuzgecKaldir(k); } };
                }));
            ana.appendChild(e('div', { sinif: 'i-secili i-dl-secili' }, [e('span', { metin: 'Süzgeç:' })].concat(etiketler.map(function(x) {
                return e('span', { sinif: 'et' }, [
                    e('button', { type: 'button', sinif: 'et-ad', title: 'Süzgeçleri göster (sırası ve kaldırma)', onclick: function() { dlAc('suzgecler'); } },
                        [e('span', { metin: x.ad + ':' }), e('b', { metin: x.metin })]),
                    e('button', { type: 'button', sinif: 'et-x', metin: '✕', title: x.ad + ' süzgecini kaldır', 'aria-label': x.ad + ' süzgecini kaldır', onclick: x.kaldir })
                ]);
            }))));
        }
        ana.appendChild(e('div', { sinif: 'i-dl-ipucu m-ek' }, [
            e('span', { metin: 'Yalnız alacaklı vekili olduğunuz İcra Dosyası türündeki açık dosyalar; talimat, iflas, MTS ve ihtiyati haciz dosyaları bu listede yok.' }),
            l && l.rol_bilinmiyor ? e('span', { metin: 'Rolü bilinmeyen ' + sayi(l.rol_bilinmiyor) + ' borçlu listede yok (dosyanın taraf bilgileri okunmamış): Dosya Listesi\'nden "Tarafları güncelle".' }) : null,
            l && l.avukat_bilinmiyor ? e('span', { metin: 'E-imza avukatının adı henüz kayıtlı değil; program UYAP\'a e-imzayla bir sonraki girişte kaydeder.' }) : null
        ]));

        // Tablo
        const basliklar = KS_SUTUN.map(function(c) {
            const id = c.k || c.s;
            const dolu = !!(c.k && f[c.k]) || (id === 'dosya_no' && !!f.yil) || (id === 'acilis' && !!(f.acilis_bas || f.acilis_son || f.acilis_yil || f.acilis_ay));
            const ok = f.sirala === c.s ? (f.yon === 'azalan' ? '↓' : '↑') : '';
            return e('th', {}, [e('button', { type: 'button', sinif: dolu ? 'dolu' : '', 'data-dl': id,
                'aria-expanded': d.ks.acik === id ? 'true' : 'false', 'aria-haspopup': 'dialog', title: c.ad + ': süzmek ya da sıralamak için tıklayın',
                onclick: function() { dlAc(c); } },
                [e('span', { metin: c.ad }), ok ? e('span', { sinif: 'ok', metin: ok }) : null, dolu ? e('span', { sinif: 'nokta', title: 'Süzgeç var' }) : null, huni()])]);
        });
        const tablo = e('table', { sinif: 'dl' }, [e('thead', {}, [e('tr', {}, basliklar)])]);
        const silik = function(m, t) { return e('span', { sinif: 'silik', metin: m, title: t }); };
        const saat = function(z) { return z ? gunMetni(z.slice(0, 10)) + ' ' + z.slice(11, 16) : ''; };
        const hucre = function(c, s) {
            switch (c.k || c.s) {
                case 'birim': return e('td', { sinif: 'tek', title: s.birim, metin: s.birim });
                case 'dosya_no': return e('td', { sinif: 'say' }, [e('span', { sinif: 'dno', metin: s.dosya_no })]);
                case 'alacakli': return e('td', { sinif: 'tek', title: s.alacaklilar.join('\n') },
                    s.alacaklilar.length ? [s.alacaklilar[0], s.alacaklilar.length > 1 ? e('span', { sinif: 'fazla', metin: '+' + (s.alacaklilar.length - 1) }) : null] : [silik('—')]);
                case 'borclu': return e('td', { sinif: 'tek', title: s.borclu, metin: s.borclu });
                case 'takip_yolu': return e('td', { sinif: 'tek', title: s.takip_turu + (s.takip_yolu ? ' › ' + s.takip_yolu : '') }, [s.takip_yolu || silik('—', 'Takip türü henüz okunmadı')]);
                case 'asama': return e('td', {}, [s.asama ? chip(s.asama, s.asama_tarihi) : silik('—', 'Takip aşaması henüz hesaplanmadı')]);
                case 'mazbata': return e('td', { sinif: 'tek', title: s.barkod ? 'Barkod ' + s.barkod : '' },
                    [s.mazbata === 'donmedi' || !s.mazbata ? silik(s.mazbata_etiket) : s.mazbata_etiket]);
                case 'kesinlesme': {
                    if (!s.kesinlesme_tarihi) return e('td', {}, [silik(s.kesinlesme_etiket || '—', 'Kesinleşme günü hesaplanamadı (tebliğ tarihi ya da mazbata yok)')]);
                    return e('td', { sinif: 'say', title: s.kesinlesme_etiket + (s.elle_tarih ? ' (gün elle girildi)' : '') }, [gunMetni(s.kesinlesme_tarihi),
                        s.kalan_gun != null ? e('div', { sinif: 'alt', metin: s.kalan_gun + ' gün kaldı' }) : null,
                        s.elle_tarih ? e('div', { sinif: 'alt', metin: 'elle girildi' }) : null]);
                }
                case 'grup': return e('td', {}, [e('span', { sinif: 'chip', stil: '--r:' + (KS_GRUP_RENK[s.grup] || '#8a877e'), metin: s.grup_etiket })]);
                case 'uyap': {
                    if (s.uyap === true) return e('td', { title: 'Son sorgu: ' + saat(s.uyap_zamani) }, ['Kesinleşti', s.uyap_gunu ? e('div', { sinif: 'alt', metin: 'görüldü ' + gunMetni(s.uyap_gunu) }) : null]);
                    if (s.uyap === false) return e('td', { title: 'Son sorgu: ' + saat(s.uyap_zamani) }, [silik('Kesinleşmedi'), e('div', { sinif: 'alt', metin: saat(s.uyap_zamani) })]);
                    return e('td', {}, [silik('—', 'UYAP\'tan henüz sorulmadı')]);
                }
                case 'talep': {
                    if (!s.talep) return e('td', {}, [silik('—', 'Kesinleştirme talebi gönderilmedi')]);
                    return e('td', { sinif: 'tek', title: s.talep.mesaj || s.talep.etiket }, [s.talep.sonuc === 'hata' ? e('span', { stil: 'color:#b84444', metin: s.talep.etiket }) : s.talep.etiket,
                        e('div', { sinif: 'alt', metin: saat(s.talep.zaman) })]);
                }
                case 'yeniden': {
                    if (!s.yeniden) return e('td', {}, [silik('—', 'Ödeme emri iade dönmemiş')]);
                    const m = s.mernis;
                    const ipucu = [m ? 'Mernis: ' + (m.adres || '(alınamadı)') : '', m ? 'İlk tebligat: ' + (m.teblig_adresi || '(okunamadı)') : '',
                        m && m.aciklama ? m.aciklama : '', s.yeniden_talep && s.yeniden_talep.mesaj ? 'Talep: ' + s.yeniden_talep.mesaj : ''].filter(Boolean).join('\n');
                    const renk = { uygun: 'var(--i-vurgu)', farkli: '#3f7fa3', hata: '#b84444', elle: '#a15c00' }[s.yeniden];
                    return e('td', { sinif: 'tek', title: ipucu }, [s.yeniden === 'sorgulanmadi' ? silik(s.yeniden_etiket) : e('span', { stil: renk ? 'color:' + renk : '', metin: s.yeniden_etiket }),
                        s.yeniden_talep ? e('div', { sinif: 'alt', metin: saat(s.yeniden_talep.zaman) }) : (m && m.zaman ? e('div', { sinif: 'alt', metin: 'Mernis ' + saat(m.zaman) }) : null)]);
                }
                case 'acilis': return e('td', { sinif: 'say' }, [s.acilis_tarihi ? gunMetni(s.acilis_tarihi) : silik('—')]);
            }
            return e('td', {});
        };
        if (!l || !l.satirlar.length) {
            tablo.appendChild(e('tbody', {}, [e('tr', {}, [e('td', { colspan: String(KS_SUTUN.length), sinif: 'silik', stil: 'text-align:center;padding:26px' },
                [l ? 'Bu ölçütlere uyan borçlu yok. ' : 'Yükleniyor…',
                 l && herhangi ? e('button', { sinif: 'i-dahasi', type: 'button', metin: 'Süzgeçleri temizle', onclick: dlTumunuTemizle }) : null])])]));
        } else {
            l.satirlar.forEach(function(s) {
                tablo.appendChild(e('tbody', { sinif: 'tik' + (d.secili && d.secili.id === s.dosya ? ' sec' : ''), onclick: function() { dosyaAc(s.dosya); } },
                    [hucreEtiketle(e('tr', {}, KS_SUTUN.map(function(c) { return hucre(c, s); })), KS_SUTUN)]));
            });
        }
        ana.appendChild(e('div', { sinif: 'i-dl-kutu' }, [tablo]));
        if (l && l.toplam > l.boyut) {
            const son = Math.max(1, Math.ceil(l.toplam / l.boyut));
            const git = function(n) { return function() { f.sayfa = n; yukle(); }; };
            ana.appendChild(e('div', { sinif: 'i-sayfa' }, [
                e('span', { metin: ((l.sayfa - 1) * l.boyut + 1) + '–' + Math.min(l.toplam, l.sayfa * l.boyut) + ' / ' + l.toplam + ' borçlu gösteriliyor' }),
                e('span', {}, [
                    e('button', { sinif: 'i-d kucuk', metin: '‹ Önceki', disabled: l.sayfa <= 1, onclick: git(l.sayfa - 1) }), ' ', l.sayfa + ' / ' + son, ' ',
                    e('button', { sinif: 'i-d kucuk', metin: 'Sonraki ›', disabled: l.sayfa >= son, onclick: git(l.sayfa + 1) })
                ])
            ]));
        }
        if (d.secili) panelCiz();
        let kendiKaydirma = false;
        if (yer > 0) { kendiKaydirma = true; ana.scrollTop = yer; setTimeout(function() { kendiKaydirma = false; }, 200); }
        ana.addEventListener('scroll', function() { if (kendiKaydirma) { kendiKaydirma = false; return; } if (d.ks.acik) dlKapat(); });
        if (d.ks.acik) dlAcilirCiz(odakVardi);
    }

    // ── Tebligatlar (19.11.2026; Python Barkod Sorgu ekranının karşılığı) ──
    // Her tebligat bir satır; borçluya bağlanmayan zarflar (banka, işveren, vekil, üçüncü kişi) da muhatap adıyla
    // listede. Her sütunda "yaz ya da seç" süzgeci (Süreçler ile aynı kutular, Tümünü seç / hariç dahil) ve başlığa
    // tıklayınca sıralama. Sıralama dosyaların sırasını belirler: bir dosyanın zarfları bölünmez, kendi içinde eskiden
    // yeniye kalır. Seçim dosya başınadır ve Süreçler ile ortaktır (toplu düğmeler seçili dosyalara işler).
    // Sütunlar: k = süzgeç anahtarı (yoksa süzgeçsiz), s = sıralama alanı, h = hücre içeriği.
    const TEB_SUTUN = [
        { ad: 'İcra dairesi', k: 'birim', s: 'birim', tekrar: true, h: function(t) { return t.birim; } },
        { ad: 'Dosya no', k: 'dosya_no', s: 'dosya_no', tekrar: true, h: function(t) {
            return [e('div', { sinif: 'dno', metin: t.dosya_no }), t.acik ? null : e('div', { sinif: 'alt', metin: t.dosya_durumu || 'Kapalı' })]; } },
        { ad: 'Dosya türü', k: 'dosya_tur', s: 'dosya_tur', kodlu: true, tekrar: true, h: function(t) { return t.dosya_tur; } },
        // Dosyadaki rolümüz (Dosya Listesi ile aynı hesap): 3. kişi / borçlu vekili olunan dosyalar süzgeçle ayrılır.
        { ad: 'Rolümüz', k: 'rol', s: 'rol', kodlu: true, tekrar: true, h: function(t) { return t.rol_etiket || e('span', { sinif: 'silik', metin: '—' }); } },
        { ad: 'Takip şekli', k: 'takip', s: 'takip', tekrar: true, h: function(t) { return t.takip || e('span', { sinif: 'silik', metin: '—', title: 'Takip türü henüz okunmadı' }); } },
        { ad: 'Alacaklı', k: 'alacakli', s: 'alacakli', tekrar: true, h: function(t) { return t.alacakli || '—'; } },
        { ad: 'Borçlu / Muhatap', k: 'borclu', s: 'borclu', h: function(t) { return t.borclu || e('span', { sinif: 'silik', metin: '(muhatap belirsiz)' }); } },
        { ad: 'Muhatap türü', k: 'muhatap', s: 'muhatap', kodlu: true, h: function(t) { return t.muhatap_etiket; } },
        { ad: 'Barkod', k: 'barkod', s: 'barkod', h: function(t) { return t.barkod || e('span', { sinif: 'silik', metin: t.elektronik ? 'E-tebligat' : '—' }); } },
        { ad: 'Grup', k: 'grup', s: 'grup', kodlu: true, h: function(t) { return t.grup; } },
        // Tür sabit gruplardandır; memurun zarfa yazdığı asıl içerik altında (aynı tür farklı yazılabiliyor).
        { ad: 'Tebligat türü', k: 'tur', s: 'tur', h: function(t) {
            const tur = t.tur || e('span', { sinif: 'silik', metin: '(türü yok)' });
            return t.zarf_icerigi && t.zarf_icerigi !== t.tur ? [tur, e('div', { sinif: 'alt uzun', title: 'Zarf içeriği: ' + t.zarf_icerigi, metin: t.zarf_icerigi })] : tur; } },
        // 89/1-2-3: belgede yazıyorsa okunur; yazmıyorsa tahmin edilmez, kullanıcı seçer (kullanıcı kararı 20.11.2026).
        { ad: '89', k: 'haciz', s: 'haciz', kodlu: true, h: function(t) { return t.haciz_turu ? hacizSirasiHucresi(t) : ''; } },
        { ad: 'Evrak tarihi', s: 'evrak', h: function(t) { return t.evrak_tarihi; } },
        { ad: 'Tebliğ / son durum', k: 'teblig', s: 'teblig', h: function(t) {
            return t.ptt_durumu ? e('div', { sinif: 'uzun', title: t.ptt_durumu, metin: t.ptt_durumu }) : e('span', { sinif: 'silik', metin: 'Henüz sorgulanmadı' }); } },
        { ad: 'Tebliğ / iade tarihi', s: 'islem', h: function(t) { return t.son_islem_tarihi; } },
        { ad: 'Teslim şekli / İade nedeni', k: 'teslim', s: 'teslim', h: function(t) { return t.teslim_sekli; } },
        { ad: 'Mazbata', k: 'mazbata', s: 'mazbata', kodlu: true, h: function(t) {
            return mazbataHucresi(t); } },
        { ad: 'Yeniden tebliğ talebi', k: 'yeniden', s: 'yeniden', kodlu: true, h: function(t) { return t.yeniden ? 'Talep edildi' : ''; } },
        { ad: 'E-tebligat', k: 'elektronik', s: 'elektronik', kodlu: true, h: function(t) { return t.elektronik ? 'E-tebligat' : 'Fiziki'; } },
        { ad: 'Kesinleşme', k: 'kesinlesme', s: 'kesinlesme', kodlu: true, h: function(t) {
            return t.kesinlesme ? t.kesinlesme : e('span', { sinif: 'silik', metin: '—', title: t.kesinlesme_neden || 'Henüz hesaplanmadı' }); } }
    ];
    const TEB_SUZGEC = TEB_SUTUN.filter(function(c) { return c.k; }).map(function(c) { return { k: c.k, ad: c.ad === '89' ? '89 sırası' : c.ad, kodlu: !!c.kodlu }; });
    function hacizSirasiHucresi(t) {
        if (t.haciz_sirasi) return e('span', { title: 'Haciz ihbarnamesi sırası', metin: '89/' + t.haciz_sirasi });
        const kap = e('span', { sinif: 'h89' });
        const sor = e('button', { sinif: 'i-d kucuk', type: 'button', metin: '?', title: 'Belgede 89 sırası yazmıyor. Seçmek için tıklayın.',
            onclick: function(ev) {
                ev.stopPropagation();
                bos(kap);
                [1, 2, 3].forEach(function(n) {
                    kap.appendChild(e('button', { sinif: 'i-d kucuk', type: 'button', metin: '89/' + n, onclick: async function(ev2) {
                        ev2.stopPropagation();
                        try { await istek('haciz_sirasi', { tebligat: t.id, sira: n }); t.haciz_sirasi = n; bildir('89/' + n + ' kaydedildi.'); yukle(); }
                        catch (x) { bildir(x.message); }
                    } }));
                });
            } });
        kap.appendChild(sor);
        return kap;
    }
    // Grup çubuğu: her grubun (diğer süzgeçler uygulanmış hâlde) tebligat sayısı; tıklayınca o gruba süzülür.
    function tebGrupCubugu(l, f, yenile) {
        const say = (l && l.secenekler && l.secenekler.grup) || [];
        if (!say.length) return null;
        const sira = ['Ödeme / İcra Emri', 'Tahliye Emri', 'Haciz İhbarnamesi (89)', 'Banka', 'Bankadan Gelen Cevap', 'Maaş', '103 Davetiyesi',
            'Kıymet Takdiri', 'Satış İlanı', 'Sıra Cetveli / Derece Kararı', 'Muhtıra', 'Borca İtiraz', 'Bilirkişi Raporu', 'Tensip Zaptı',
            'İcra Dairesi Genel Yazı', 'Diğer'];
        const liste = say.slice().sort(function(a, b) { return sira.indexOf(a.deger) - sira.indexOf(b.deger); });
        const secili = f.grup ? f.grup.split('\n') : [];
        const cip = function(ad, adet, aktif, fn) {
            return e('button', { sinif: 'i-grup' + (aktif ? ' aktif' : ''), type: 'button', onclick: fn }, [ad, e('b', { metin: ' ' + adet.toLocaleString('tr-TR') })]);
        };
        const toplam = say.reduce(function(s, x) { return s + x.adet; }, 0);
        return e('div', { sinif: 'i-gruplar', 'aria-label': 'Tebligat grupları' }, [cip('Tümü', toplam, !secili.length, function() { f.grup = ''; yenile(); })]
            .concat(liste.map(function(x) {
                return cip(x.etiket || x.deger, x.adet, secili.indexOf(x.deger) >= 0, function() { f.grup = secili.length === 1 && secili[0] === x.deger ? '' : x.deger; yenile(); });
            })));
    }
    function tebFiltreBos() {
        const f = { q: '', durum: 'acik', acilis_bas: '', acilis_son: '', evrak_bas: '', evrak_son: '', islem_bas: '', islem_son: '',
                    sirala: '', yon: 'artan', sayfa: 1 };
        TEB_SUZGEC.forEach(function(z) { f[z.k] = ''; });
        return f;
    }
    d.teb = { filtre: tebFiltreBos(), liste: null };
    function tebSorgusu(sayfasiz) {
        const f = d.teb.filtre, s = {};
        ['q', 'durum', 'acilis_bas', 'acilis_son', 'evrak_bas', 'evrak_son', 'islem_bas', 'islem_son', 'sirala', 'yon'].forEach(function(k) { if (f[k]) s[k] = f[k]; });
        TEB_SUZGEC.forEach(function(z) { if (f[z.k]) s[z.k] = f[z.k]; });
        if (!sayfasiz) { s.sayfa = String(f.sayfa); s.boyut = '100'; }
        if (d.secListe) s.liste = String(d.secListe);
        return new URLSearchParams(s).toString();
    }
    let tebAraZaman = null;
    function tebCiz() {
        bos(govde);
        const ana = e('div', { sinif: 'i-ana' });
        govde.appendChild(ana);
        const f = d.teb.filtre, l = d.teb.liste;
        const yenile = function() { f.sayfa = 1; yukle(); };
        const ara = e('input', { type: 'search', placeholder: 'Dosya no, daire, borçlu / muhatap, alacaklı ya da barkod ara', value: f.q,
            oninput: function() { clearTimeout(tebAraZaman); tebAraZaman = setTimeout(function() { f.q = ara.value; yenile(); }, 300); } });
        const durumSec = e('select', { 'aria-label': 'Dosya durumu', title: 'UYAP\'taki dosya durumu' },
            DURUMLAR.map(function(x) { const o = e('option', { value: x[0], metin: x[1] }); if (f.durum === x[0]) o.selected = true; return o; }));
        durumSec.addEventListener('change', function() { f.durum = durumSec.value; yenile(); });
        const aralik = function(k, ad) {
            const kutu = function(uc, b) {
                const t = e('input', { type: 'date', value: f[k + uc], title: ad + ': ' + b, 'aria-label': ad + ': ' + b });
                t.addEventListener('change', function() { f[k + uc] = t.value; yenile(); });
                return t;
            };
            return e('label', { sinif: 'i-onay', title: ad + ' aralığı (iki gün dahil)' }, [ad, kutu('_bas', 'başlangıç'), '–', kutu('_son', 'bitiş')]);
        };
        const excel = e('button', { sinif: 'i-d', type: 'button', metin: 'Excel\'e aktar', title: 'Süzülmüş ve sıralı listenin tamamı (bütün sayfalar), her tebligat bir satır; dosya bu cihaza iner',
            onclick: function() { tebExcel(excel); } });
        const turEs = e('button', { sinif: 'i-d', type: 'button', metin: 'Tür eşleştir', title: 'Şablon dışı tebligat türlerini bir türe bağlayın (bir kez sorulur)',
            onclick: function() { turEslestirAc(); } });
        ana.appendChild(e('div', { sinif: 'i-arac' }, [ara, mAracDugmesi(ana), mEk(durumSec), e('div', { sinif: 'i-bosluk' }), mEk(gtDugme()), mEk(excel), mEk(turEs)]));
        ana.appendChild(e('div', { sinif: 'i-arac m-ek' }, [aralik('acilis', 'Açılış'), aralik('evrak', 'Evrak tarihi'), aralik('islem', 'Tebliğ / iade tarihi')]));
        if (gtGorunsun()) ana.appendChild(gtPaneli());
        if (f.q && document.activeElement !== ara) setTimeout(function() { ara.focus(); ara.setSelectionRange(ara.value.length, ara.value.length); }, 0);
        seciliCiz(ana);
        if (dar()) ana.appendChild(e('div', { sinif: 'i-suzgecler m-ek' }, TEB_SUZGEC.map(function(z) { return suzgecKutusu(z, true); })));
        const gc = tebGrupCubugu(l, f, yenile);
        if (gc) ana.appendChild(gc);
        ana.appendChild(tebSecimCubugu(l));
        const baslik = function(c) {
            const z = c.k && TEB_SUZGEC.find(function(x) { return x.k === c.k; });
            const ok = f.sirala === c.s ? (f.yon === 'azalan' ? ' ▼' : ' ▲') : '';
            const ad = e('span', { sinif: 'i-sirala', role: 'button', tabindex: '0', title: 'Sıralamak için tıklayın (dosyanın tebligatları bölünmez)', metin: c.ad + ok,
                onclick: function() {
                    if (f.sirala === c.s) f.yon = f.yon === 'azalan' ? 'artan' : 'azalan'; else { f.sirala = c.s; f.yon = 'artan'; }
                    yenile();
                } });
            return e('th', { sinif: z ? 'suz suz-' + z.k : '' }, [ad, z && !dar() ? suzgecKutusu(z, false) : null]);
        };
        const tablo = e('table', { sinif: 'liste teb' }, [e('thead', {}, [e('tr', {}, [e('th', {})].concat(TEB_SUTUN.map(baslik)))])]);
        if (!l || !l.satirlar.length) {
            tablo.appendChild(e('tbody', {}, [e('tr', {}, [e('td', { colspan: String(TEB_SUTUN.length + 1), sinif: 'silik', stil: 'text-align:center;padding:22px',
                metin: 'Bu ölçütlere uyan tebligat yok.' })])]));
            ana.appendChild(e('div', { sinif: 'i-dl-kutu' }, [tablo]));
        } else {
            // Ardışık aynı dosyanın satırları bir grup (tbody): dosya bilgileri ilk satırdan sonra soluk.
            let grup = null, onceki = null;
            l.satirlar.forEach(function(t) {
                if (t.dosya !== onceki) {
                    grup = e('tbody', { sinif: 'tik teb-dosya' + (d.secili && d.secili.id === t.dosya ? ' sec' : ''), onclick: (function(id) { return function() { dosyaAc(id); }; })(t.dosya) });
                    tablo.appendChild(grup);
                }
                const ilk = t.dosya !== onceki;
                onceki = t.dosya;
                const kutu = e('input', { type: 'checkbox', sinif: 'i-dosya-sec', 'data-dosya': String(t.dosya), checked: d.secim.has(t.dosya),
                    'aria-label': 'Dosyayı seç: ' + t.birim + ' ' + t.dosya_no, onclick: function(ev) { ev.stopPropagation(); } });
                kutu.addEventListener('change', function() { if (kutu.checked) d.secim.add(t.dosya); else d.secim.delete(t.dosya); secimYaz(); });
                grup.appendChild(e('tr', { sinif: ilk ? '' : 'ek' }, [e('td', {}, [kutu])].concat(TEB_SUTUN.map(function(c) {
                    const ic = c.h(t);
                    return e('td', { sinif: !ilk && c.tekrar ? 'tekrar' : '' }, Array.isArray(ic) ? ic : [ic == null ? '' : ic]);
                }))));
            });
            ana.appendChild(e('div', { sinif: 'i-dl-kutu' }, [tablo]));
            const son = Math.max(1, Math.ceil(l.toplam / l.boyut));
            const git = function(n) { return function() { f.sayfa = n; yukle(); }; };
            ana.appendChild(e('div', { sinif: 'i-sayfa' }, [
                e('span', { metin: l.toplam + ' tebligat (' + l.dosya_sayisi + ' dosya) · ' + ((l.sayfa - 1) * l.boyut + 1) + '–' + Math.min(l.toplam, l.sayfa * l.boyut) + ' gösteriliyor' }),
                son > 1 ? e('span', {}, [
                    e('button', { sinif: 'i-d kucuk', metin: '‹ Önceki', disabled: l.sayfa <= 1, onclick: git(l.sayfa - 1) }), ' ', l.sayfa + ' / ' + son, ' ',
                    e('button', { sinif: 'i-d kucuk', metin: 'Sonraki ›', disabled: l.sayfa >= son, onclick: git(l.sayfa + 1) })
                ]) : null
            ]));
        }
        ana.addEventListener('scroll', acilirKaydir);
        if (d.kutu) {
            const kutu = kok.querySelector('.i-kutu-s[data-k="' + d.kutu + '"] input');
            if (kutu) { kutu.focus(); kutu.setSelectionRange(kutu.value.length, kutu.value.length); acilirCiz(); }
            else acilirKapat();
        }
        if (d.secili) panelCiz();
    }
    function tebSecimCubugu(l) {
        const toplam = l ? l.dosya_sayisi : 0;
        const tumu = e('button', { sinif: 'i-d kucuk', type: 'button', metin: 'Tümünü seç (' + toplam + ' dosya)', disabled: !toplam,
            title: 'Süzgeçteki bütün tebligatların dosyalarını seçer (bütün sayfalar); önceki seçime eklenir',
            onclick: async function() {
                tumu.disabled = true;
                try { (await istek('tebligat_dosya_kimlikleri?' + tebSorgusu(true))).forEach(function(x) { d.secim.add(x); }); secimYaz(); }
                catch (x) { bildir(x.message); }
                tumu.disabled = false;
            } });
        return e('div', { sinif: 'i-secim', stil: 'display:flex;gap:8px;align-items:center;flex-wrap:wrap;margin:6px 0' }, [
            tumu,
            e('button', { sinif: 'i-d kucuk i-secim-birak', type: 'button', metin: 'Tümünü bırak', disabled: !d.secim.size, onclick: function() { d.secim.clear(); secimYaz(); } }),
            e('span', { sinif: 'i-secim-sayi alt', metin: d.secim.size ? d.secim.size + ' dosya seçili' : 'Seçili dosya yok' }),
            e('span', { sinif: 'alt', metin: '· Seçim dosya başınadır ve Süreçler ile ortaktır; "Toplu tarama" seçim varsa yalnız seçili, yoksa süzgeçteki dosyalara işler.' })
        ]);
    }
    async function tebExcel(dugme) {
        dugme.disabled = true;
        try {
            const r = await fetch(UC + 'tebligatlar_excel?' + tebSorgusu(true), { cache: 'no-store', credentials: 'same-origin' });
            if (!r.ok) { let m = 'Excel hazırlanamadı (' + r.status + ').'; try { m = (await r.json()).error || m; } catch (x) {} throw new Error(m); }
            const ad = (/filename="([^"]+)"/.exec(r.headers.get('Content-Disposition') || '') || [])[1] || 'Tebligatlar.xlsx';
            const url = URL.createObjectURL(await r.blob());
            const a = e('a', { href: url, download: ad, 'data-uyap-internal': '1', stil: 'display:none' });
            document.body.appendChild(a); a.click(); a.remove();
            setTimeout(function() { URL.revokeObjectURL(url); }, 30000);
            bildir('Excel indirildi: ' + ad);
        } catch (x) { bildir(x.message); }
        dugme.disabled = false;
    }

    // ── Tür eşleştir: şablon dışı tebligat türü yazılışlarını gruplar hâlinde bir türe bağlar ──
    // Tarih ve yazım farkları aynı grupta. Kaydedince mevcut kayıtlar düzeltilir, borçlular yeniden hesaplanır,
    // sonraki taramalar aynı yazılışı kendiliğinden o türe çevirir; eşleştirilen yazılış bir daha sorulmaz.
    async function turEslestirAc() {
        let v;
        try { v = await istek('tur_eslestirme'); } catch (x) { bildir(x.message); return; }
        const secimler = {};      // anahtar → tür
        const isaretli = new Set();
        const ortu = e('div', { sinif: 'i-ortu', role: 'dialog', 'aria-modal': 'true', 'aria-label': 'Tebligat türü eşleştir' });
        const kapat = function() { ortu.remove(); };
        ortu.addEventListener('mousedown', function(ev) { if (ev.target === ortu) kapat(); });
        const pencere = e('div', { sinif: 'i-pencere' });
        ortu.appendChild(pencere);
        const govdeT = e('tbody');
        const durum = e('span', { sinif: 'alt' });
        const turKutu = e('input', { type: 'text', list: 'cu-tur-secenek', placeholder: 'Tür seçin ya da yeni tür yazın', 'aria-label': 'Atanacak tür', stil: 'width:280px' });
        const veriListesi = e('datalist', { id: 'cu-tur-secenek' }, v.turler.map(function(t) { return e('option', { value: t }); }));
        const ciz = function() {
            bos(govdeT);
            if (!v.gruplar.length) govdeT.appendChild(e('tr', {}, [e('td', { colspan: '7', sinif: 'silik', stil: 'text-align:center;padding:18px', metin: 'Eşleştirilecek şablon dışı tür yok.' })]));
            v.gruplar.forEach(function(g) {
                const k = e('input', { type: 'checkbox', checked: isaretli.has(g.anahtar), 'aria-label': 'Grubu seç: ' + g.ornek });
                k.addEventListener('change', function() { if (k.checked) isaretli.add(g.anahtar); else isaretli.delete(g.anahtar); durumYaz2(); });
                govdeT.appendChild(e('tr', {}, [e('td', {}, [k]),
                    e('td', { title: g.yazilislar.map(function(y) { return y.yazilis + ' (' + y.adet + ')'; }).join('\n'), metin: g.ornek }),
                    e('td', { metin: String(g.adet) }), e('td', { metin: String(g.cesit) }), e('td', { metin: g.kaynak }),
                    e('td', { sinif: 'alt', metin: g.ornek_dosya }),
                    e('td', {}, [secimler[g.anahtar] ? e('b', { metin: secimler[g.anahtar] }) : e('span', { sinif: 'silik', metin: '—' })])]));
            });
            durumYaz2();
        };
        const durumYaz2 = function() {
            durum.textContent = v.gruplar.length + ' grup · ' + isaretli.size + ' seçili · ' + Object.keys(secimler).length + ' gruba tür atandı';
        };
        const ata = e('button', { sinif: 'i-d', type: 'button', metin: 'Seçili gruplara ata', onclick: function() {
            const tur = turKutu.value.trim();
            if (!tur) { bildir('Önce bir tür seçin ya da yazın.'); return; }
            if (!isaretli.size) { bildir('Önce listeden grup seçin.'); return; }
            isaretli.forEach(function(a) { secimler[a] = tur; });
            isaretli.clear();
            ciz();
        } });
        const kaydet = e('button', { sinif: 'i-d ana', type: 'button', metin: 'Kaydet ve uygula', onclick: async function() {
            const liste = Object.keys(secimler).map(function(a) { return { anahtar: a, tur: secimler[a] }; });
            if (!liste.length) { bildir('Henüz hiçbir gruba tür atanmadı.'); return; }
            kaydet.disabled = true;
            try {
                const s = await istek('tur_eslestir', { eslesmeler: liste });
                bildir(s.eslesme + ' eşleştirme kaydedildi; ' + s.degisen + ' tebligatın türü düzeltildi' + (s.guncellenen ? ', ' + s.guncellenen + ' borçlunun durumu güncellendi.' : '.'));
                kapat();
                yukle();
            } catch (x) { bildir(x.message); kaydet.disabled = false; }
        } });
        pencere.appendChild(e('div', { sinif: 'i-gt-ust' }, [e('b', { metin: 'Şablon dışı tebligat türleri' }), e('div', { sinif: 'i-bosluk' }),
            e('button', { sinif: 'i-d kucuk', type: 'button', metin: 'Kapat', onclick: kapat })]));
        pencere.appendChild(e('p', { sinif: 'alt', stil: 'margin:0 0 8px', metin: 'Tarih ve yazım farkları aynı grupta toplanır. Grupları işaretleyip türü seçin ya da yeni bir tür adı yazın, "Seçili gruplara ata" deyin; bitince "Kaydet ve uygula". Eşleştirilen yazılış bir daha sorulmaz.' }));
        pencere.appendChild(e('div', { sinif: 'i-pencere-ic' }, [e('table', {}, [e('thead', {}, [e('tr', {}, ['', 'En sık yazılış', 'Kayıt', 'Yazılış', 'Kaynak', 'Örnek dosya', 'Seçilen tür'].map(function(b) { return e('th', { metin: b }); }))]), govdeT])]));
        pencere.appendChild(e('div', { sinif: 'i-gt-form' }, [turKutu, veriListesi, ata, e('div', { sinif: 'i-bosluk' }), durum, kaydet]));
        kok.appendChild(ortu);
        ciz();
    }

    // ── Kesinleşme süreleri ──
    // Süre takip türü → yolu → şekli ADLARIYLA seçilir (19.11.2026, kullanıcı isteği). Kodlar UYAP'ındır ve yalnız
    // üstündekiyle anlamlıdır (şekil "0" ilamsızda Örnek 7, ilamlıda Örnek 4-5); ekranda gösterilmez. Seçenekler:
    // UYAP takip açılış ekranının listesi + dosyalarda görülen türler (dosya sayılarıyla) — sunucu `agac` olarak verir.
    function sekliKisa(ad) { return String(ad || '').replace(/\s*-\s*Eski\s*No\s*:?\s*[\d-]+\s*$/i, ''); }
    function takipAdi(ad, kod, sekil) { return (sekil ? sekliKisa(ad) : ad) || ('Kod ' + kod + ' (adı bilinmiyor)'); }
    // Bir kuralın / dağılım satırının takip türü yolu: "İlamsız Takip › Genel Haciz Yoluyla Takip › ÖRNEK: 7 …".
    function takipYolu(turu, turuAd, yolu, yoluAd, sekli, sekliAd) {
        if (!turu) return 'Bütün takipler (varsayılan)';
        const p = [takipAdi(turuAd, turu)];
        p.push(yolu ? takipAdi(yoluAd, yolu) : 'bütün yollar');
        if (yolu) p.push(sekli ? takipAdi(sekliAd, sekli, true) : 'bütün şekiller');
        return p.join(' › ');
    }
    let listeZaman = null;
    function takipListesiAl(dugme) {
        return async function() {
            dugme.disabled = true;
            let t;
            try { t = await gtIstek('', { mod: 'takip_listesi' }); }
            catch (x) { bildir(x.message); dugme.disabled = false; return; }
            if (!t || t.mod !== 'takip_listesi') { bildir('Süren bir toplu tarama var; bitince yeniden deneyin.'); dugme.disabled = false; return; }
            dugme.textContent = 'Alınıyor…';
            const bekle = async function() {
                let s;
                try { s = await gtIstek('/' + t.id); }
                catch (x) { bildir(x.message); dugme.disabled = false; dugme.textContent = 'UYAP\'tan takip türü listesini al'; return; }
                if (GT_SON.indexOf(s.durum) < 0) { listeZaman = setTimeout(bekle, 2000); return; }
                bildir(s.mesaj || 'Bitti.');
                if (d.sekme === 'sure') yukle();
            };
            clearTimeout(listeZaman);
            listeZaman = setTimeout(bekle, 1500);
        };
    }
    function sureCiz(v) {
        bos(govde);
        const ana = e('div', { sinif: 'i-ana' });
        govde.appendChild(ana);
        const agac = v.agac || [];
        const listeAl = e('button', { sinif: 'i-d kucuk', type: 'button', metin: 'UYAP\'tan takip türü listesini al',
            title: 'UYAP takip açılış ekranındaki takip türü, yolu ve şekli listesini alır (birkaç on istek, ofis bilgisayarından)' });
        listeAl.addEventListener('click', takipListesiAl(listeAl));
        const kutu = e('div', { sinif: 'i-kutu' }, [e('h3', { metin: 'Kesinleşme süreleri' }),
            e('p', { metin: 'Kesinleşme süresi, icra işleminin yapılabileceği gündür: itiraz süresi ile ödeme süresinden uzun olanı (ör. kambiyo 5 gün itiraz, 10 gün ödeme → 10 gün; kira ödeme emri → 30 gün). Hazır kurallar programla gelir; değiştirdiğiniz kurala program dokunmaz, "Varsayılana dön" ile geri alınır. Süre boşsa o türde kesinleşme yoktur. "İtiraz takibi durdurmaz" türlerde (ilamlı takip, kambiyo haciz yolu) elle işaretlenen itiraz kesinleşmeyi durdurmaz; UYAP\'ta "Durdurulmuş" görünen dosya her türde durur. Bir dosyaya en özel uyan kural uygulanır; takip türü okunmamış dosyalara varsayılan uygulanır. Kural değişince bütün borçlular yeniden hesaplanır.' }),
            e('div', { sinif: 'i-eylem', stil: 'margin:0 0 6px' }, [listeAl, e('span', { sinif: 'alt', metin: v.liste_zamani
                ? 'UYAP listesi ' + gunMetni(v.liste_zamani) + v.liste_zamani.slice(10) + ' tarihinde alındı.'
                : 'UYAP listesi henüz alınmadı; açılır listelerde yalnız dosyalarda görülen türler var.' })])]);
        ana.appendChild(kutu);
        let duzenlenen = null;
        const form = e('div');
        function formCiz(k) {
            bos(form);
            duzenlenen = k ? k.id : null;
            const sec = { tur: (k && k.takibin_turu) || '', yol: (k && k.takibin_yolu) || '', sek: (k && k.takibin_sekli) || '' };
            let adElle = !!(k && k.ad);
            const turSec = e('select', { 'aria-label': 'Takip türü', stil: 'max-width:240px' });
            const yolSec = e('select', { 'aria-label': 'Takip yolu', stil: 'max-width:300px' });
            const sekSec = e('select', { 'aria-label': 'Takip şekli', stil: 'max-width:420px' });
            const ad = e('input', { type: 'text', placeholder: 'Ad', value: (k && k.ad) || '', stil: 'width:260px', 'aria-label': 'Kuralın adı' });
            ad.addEventListener('input', function() { adElle = !!ad.value.trim(); });
            const gun = e('input', { type: 'number', min: '1', max: '365', placeholder: 'Kesinleşme günü (boş: yok)', value: k && k.gun != null ? String(k.gun) : '', stil: 'width:190px', 'aria-label': 'Kesinleşme süresi (gün)', title: 'İtiraz ve ödeme sürelerinden uzun olanı' });
            const itirazGun = e('input', { type: 'number', min: '1', max: '365', placeholder: 'İtiraz günü', value: k && k.itiraz_gun != null ? String(k.itiraz_gun) : '', stil: 'width:120px', 'aria-label': 'İtiraz süresi (gün)', title: 'Bilgi amaçlı; hesapta kesinleşme süresi kullanılır' });
            const durdurur = e('input', { type: 'checkbox' }); durdurur.checked = !k || k.itiraz_durdurur !== false;
            const dayanak = e('input', { type: 'text', placeholder: 'Dayanak (ör. İİK 168–170)', value: (k && k.dayanak) || '', stil: 'width:200px', 'aria-label': 'Dayanak' });
            const aktif = e('input', { type: 'checkbox' }); aktif.checked = !k || k.aktif;
            const secenekMetni = function(x, sekil) { return takipAdi(x.ad, x.kod, sekil) + (x.adet ? ' · ' + x.adet + ' dosya' : ''); };
            const doldur = function(el, liste, secili, hepsi, sekil) {
                bos(el);
                el.appendChild(e('option', { value: '', metin: hepsi }));
                liste.forEach(function(x) { el.appendChild(e('option', { value: x.kod, metin: secenekMetni(x, sekil), title: x.ad || '' })); });
                if (secili && !liste.some(function(x) { return x.kod === secili; })) el.appendChild(e('option', { value: secili, metin: 'Kod ' + secili + ' (listede yok)' }));
                el.value = secili;
            };
            const adOner = function(tur, yol, sek) {
                if (!sec.tur) return 'Varsayılan';
                const p = [tur ? takipAdi(tur.ad, tur.kod) : 'Kod ' + sec.tur];
                if (sec.yol) p.push(yol ? takipAdi(yol.ad, yol.kod) : 'Kod ' + sec.yol);
                if (sec.sek) p.push(sek && sek.ornek_no ? 'Örnek ' + sek.ornek_no : sek ? takipAdi(sek.ad, sek.kod, true) : 'Kod ' + sec.sek);
                return p.join(' · ');
            };
            const yenile = function() {
                const tur = agac.find(function(x) { return x.kod === sec.tur; });
                const yol = tur && tur.yollar.find(function(x) { return x.kod === sec.yol; });
                const sek = yol && yol.sekiller.find(function(x) { return x.kod === sec.sek; });
                doldur(turSec, agac, sec.tur, 'Bütün takip türleri (varsayılan)');
                doldur(yolSec, tur ? tur.yollar : [], sec.yol, sec.tur ? 'Bütün yollar' : '— önce takip türü —');
                doldur(sekSec, yol ? yol.sekiller : [], sec.sek, sec.yol ? 'Bütün şekiller' : '— önce takip yolu —', true);
                yolSec.disabled = !sec.tur;
                sekSec.disabled = !sec.yol;
                if (!adElle) ad.value = adOner(tur, yol, sek);
            };
            turSec.addEventListener('change', function() { sec.tur = turSec.value; sec.yol = ''; sec.sek = ''; yenile(); });
            yolSec.addEventListener('change', function() { sec.yol = yolSec.value; sec.sek = ''; yenile(); });
            sekSec.addEventListener('change', function() { sec.sek = sekSec.value; yenile(); });
            yenile();
            form.appendChild(e('div', { sinif: 'i-form' }, [turSec, yolSec, sekSec]));
            form.appendChild(e('div', { sinif: 'i-form', stil: 'margin-top:-4px' }, [ad, gun, itirazGun, e('label', { sinif: 'i-onay' }, [durdurur, 'İtiraz takibi durdurur']), dayanak, e('label', { sinif: 'i-onay' }, [aktif, 'Etkin']),
                e('button', { sinif: 'i-d ana', metin: k && k.id ? 'Kaydet' : 'Ekle', onclick: async function() {
                    try {
                        const s = await istek('kural', { id: duzenlenen, takibin_turu: sec.tur, takibin_yolu: sec.yol, takibin_sekli: sec.sek,
                                                         ad: ad.value, gun: gun.value ? Number(gun.value) : null, aktif: aktif.checked,
                                                         itiraz_gun: itirazGun.value ? Number(itirazGun.value) : null, itiraz_durdurur: durdurur.checked,
                                                         dayanak: dayanak.value });
                        bildir('Kaydedildi' + (s.guncellenen ? '; ' + s.guncellenen + ' borçlunun durumu güncellendi.' : '.'));
                        yukle();
                    } catch (x) { bildir(x.message); }
                } }),
                k ? e('button', { sinif: 'i-d', metin: 'Vazgeç', onclick: function() { formCiz(null); } }) : null]));
        }
        if (!v.kurallar.length) kutu.appendChild(e('div', { sinif: 'i-sarı', metin: 'Henüz süre girilmedi: hiçbir dosyada kesinleşme hesaplanmaz. Önce varsayılan süreyi (takip türü seçmeden) ekleyin.' }));
        else {
            kutu.appendChild(e('table', {}, [
                e('thead', {}, [e('tr', {}, ['Ad', 'Takip türü › yolu › şekli', 'Dosya', 'Kesinleşme', 'İtiraz', 'İtiraz takibi', 'Dayanak', '', ''].map(function(b) { return e('th', { metin: b }); }))]),
                e('tbody', {}, v.kurallar.map(function(k) {
                    const hazir = k.varsayilan;
                    let ikinci;
                    if (hazir) {
                        // Hazır kural silinmez; değiştirildiyse varsayılana döner.
                        ikinci = k.elle_degisti || !k.aktif ? e('button', { sinif: 'i-d kucuk', metin: 'Varsayılana dön',
                            title: 'Hazır değer: ' + (hazir.gun ? hazir.gun + ' gün' : 'kesinleşme yok') + (hazir.itiraz_gun ? ', itiraz ' + hazir.itiraz_gun + ' gün' : '') + (hazir.itiraz_durdurur ? '' : ', itiraz durdurmaz'),
                            onclick: async function() {
                                try { const s = await istek('kural_varsayilan', { id: k.id }); bildir('Varsayılana döndü' + (s.guncellenen ? '; ' + s.guncellenen + ' borçlunun durumu güncellendi.' : '.')); yukle(); }
                                catch (x) { bildir(x.message); }
                            } }) : null;
                    } else {
                        ikinci = e('button', { sinif: 'i-d kucuk tehlike', metin: 'Sil' });
                        ikinci.addEventListener('click', async function() {
                            if (!ikinci.dataset.onay) { ikinci.dataset.onay = '1'; ikinci.textContent = 'Emin misiniz?'; setTimeout(function() { delete ikinci.dataset.onay; ikinci.textContent = 'Sil'; }, 4000); return; }
                            try { await istek('kural_sil', { id: k.id }); bildir('Silindi.'); yukle(); } catch (x) { bildir(x.message); }
                        });
                    }
                    const rozet = !k.aktif ? 'Devre dışı' : !hazir ? 'Sizin kuralınız' : k.elle_degisti ? 'Değiştirildi' : 'Varsayılan';
                    return e('tr', {}, [
                        e('td', {}, [k.ad || (k.takibin_turu ? '' : 'Varsayılan'), e('div', { sinif: 'alt', metin: rozet })]),
                        e('td', { metin: takipYolu(k.takibin_turu, k.turu_ad, k.takibin_yolu, k.yolu_ad, k.takibin_sekli, k.sekli_ad) }),
                        e('td', { metin: k.adet ? String(k.adet) : '—' }),
                        e('td', {}, [e('span', { sinif: 'gun', metin: k.gun ? k.gun + ' gün' : 'Yok' })]),
                        e('td', { metin: k.itiraz_gun ? k.itiraz_gun + ' gün' : '—' }),
                        e('td', { metin: k.gun ? (k.itiraz_durdurur ? 'Durdurur' : 'Durdurmaz') : '—' }),
                        e('td', { sinif: 'alt', metin: k.dayanak || '' }),
                        e('td', { stil: 'text-align:right;white-space:nowrap' }, [e('button', { sinif: 'i-d kucuk', metin: 'Düzenle', onclick: function() { formCiz(k); } })]),
                        e('td', { stil: 'text-align:right;white-space:nowrap' }, [ikinci])
                    ]);
                }))
            ]));
        }
        kutu.appendChild(form);
        formCiz(null);
        // Veritabanındaki takip türleri: kural girerken hangisinin kaç dosyada olduğu
        const dag = e('div', { sinif: 'i-kutu', stil: 'margin-top:14px' }, [e('h3', { metin: 'Veritabanındaki takip türleri' }),
            e('p', { metin: 'İcra dosyalarının UYAP\'tan okunan takip türleri ve dosya sayıları. Birine süre girmek için "Süre gir"e basın.' })]);
        if (!v.dagilim.length) dag.appendChild(e('div', { sinif: 'alt', metin: 'Henüz takip türü okunmuş dosya yok.' }));
        else dag.appendChild(e('table', {}, [
            e('thead', {}, [e('tr', {}, ['Takip türü › yolu › şekli', 'Dosya', 'Örnek dosya', ''].map(function(b) { return e('th', { metin: b }); }))]),
            e('tbody', {}, v.dagilim.map(function(x) {
                return e('tr', {}, [e('td', { metin: takipYolu(x.turu, x.turu_ad, x.yolu, x.yolu_ad, x.sekli, x.sekli_ad) }),
                    e('td', { metin: String(x.adet) }), e('td', { sinif: 'alt', metin: x.ornek || '' }),
                    e('td', { stil: 'text-align:right' }, [e('button', { sinif: 'i-d kucuk', metin: 'Süre gir', onclick: function() {
                        formCiz({ id: null, takibin_turu: x.turu, takibin_yolu: x.yolu, takibin_sekli: x.sekli, ad: '', gun: null, itiraz_gun: null, itiraz_durdurur: true, dayanak: '', aktif: true });
                        form.scrollIntoView({ block: 'center' });
                    } })])]);
            }))
        ]));
        if (v.takip_turu_bilinmeyen) dag.appendChild(e('div', { sinif: 'alt', stil: 'margin-top:8px', metin: 'Takip türü henüz okunmamış icra dosyası: ' + v.takip_turu_bilinmeyen + ' (bunlara varsayılan süre uygulanır). Süreçler › Toplu tarama › "Takip türü bilgisi" ile alınır.' }));
        ana.appendChild(dag);
    }

    // ── Resmî tatiller ──
    function tatilCiz(liste) {
        bos(govde);
        const ana = e('div', { sinif: 'i-ana' });
        govde.appendChild(ana);
        const kutu = e('div', { sinif: 'i-kutu', stil: 'max-width:640px' }, [e('h3', { metin: 'Resmî tatiller' }),
            e('p', { metin: 'Kesinleşme süresinin son günü hafta sonuna ya da buradaki bir güne denk gelirse ilk iş gününe kayar. Dinî bayramlar her yıl Diyanet ilanına göre girilmeli. Değişince bütün borçlular yeniden hesaplanır.' })]);
        ana.appendChild(kutu);
        const tarih = e('input', { type: 'date' });
        const aciklama = e('input', { type: 'text', placeholder: 'Açıklama (ör. Cumhuriyet Bayramı)', stil: 'flex:1;min-width:160px' });
        kutu.appendChild(e('div', { sinif: 'i-form' }, [tarih, aciklama, e('button', { sinif: 'i-d ana', metin: 'Ekle', onclick: async function() {
            if (!tarih.value) { bildir('Tarih seçin.'); return; }
            try { await istek('tatil', { tarih: tarih.value, aciklama: aciklama.value }); bildir('Eklendi.'); yukle(); } catch (x) { bildir(x.message); }
        } })]));
        if (!liste.length) { kutu.appendChild(e('div', { sinif: 'alt', metin: 'Kayıtlı tatil yok.' })); return; }
        kutu.appendChild(e('table', {}, [e('tbody', {}, liste.map(function(t) {
            const sil = e('button', { sinif: 'i-d kucuk tehlike', metin: 'Sil', onclick: async function() {
                try { await istek('tatil_sil', { tarih: t.tarih }); bildir('Silindi.'); yukle(); } catch (x) { bildir(x.message); }
            } });
            return e('tr', {}, [e('td', { metin: gunMetni(t.tarih), stil: 'white-space:nowrap' }), e('td', { metin: t.aciklama || '' }),
                                e('td', { stil: 'text-align:right' }, [sil])]);
        }))]));
    }

    // ── Bağlantılar ──
    // Menüde başka bir bağlantıya basınca sayfa görev çubuğuna küçülür (UYAP'ın kendi sayfası görünür).
    document.addEventListener('click', function(ev) {
        if (gorunum !== 'acik') return;
        const a = ev.target.closest && ev.target.closest('#av-sidebar-wrapper a, #av-sidebar-wrapper button.sideLink');
        if (a && !a.closest('#cu-menu-icra') && !a.closest('.fav-menu-icon')) kucult();
    }, true);
    document.addEventListener('keydown', function(ev) {
        if (onde() && ev.key === 'Escape' && d.secili && !ev.target.closest('input,select')) { d.secili = null; aktifCiz(); }
    });
    // Bu sayfadan açılan dosya kartı küçültüldü / kapatıldı: sayfa küçükse yeniden öne gelir (açıksa zaten kartın
    // altında duruyor, kart gidince görünür).
    function kartDinle() {
        if (!window.__cuKartKapaninca) return false;
        window.__cuKartKapaninca(function(anahtar) {
            if (!icradanAcilan.has(anahtar)) return;
            icradanAcilan.delete(anahtar);
            if (gorunum === 'kucuk') ac(false);
        });
        return true;
    }
    if (!kartDinle()) setTimeout(kartDinle, 1000);
    window.addEventListener('resize', yerlestir);
    setInterval(function() { menuGirisi(); yerlestir(); gorevGuncelle(false); }, 700);
    menuGirisi();
    try {
        const v = sessionStorage.getItem(ACIK_ANAHTAR);
        if (v === '1') setTimeout(function() { ac(false); }, 300);
        else if (v === 'k') { gorunum = 'kucuk'; setTimeout(function() { gorevGuncelle(true); }, 300); }
    } catch (x) {}
    window.__cuIcraTakipAc = function() { ac(true); };
})();
