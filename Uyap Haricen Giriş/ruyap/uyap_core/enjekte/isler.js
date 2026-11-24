// ═══════════════════════════════════════════════════════════════════════════════════════
// ÇOKLU UYAP — İŞ LİSTESİ SAYFASI (2026-11-16, plan L4)
// ═══════════════════════════════════════════════════════════════════════════════════════
// Buluttaki İş Listesi'nin yerini alır: veri ofis bilgisayarındaki büro veritabanında, uçlar
// /__cu__/veri/is/… (src/is_api.rs). Her istek kişi adına çalışır (bu bilgisayar: programa girişli hesap;
// tünel: imzalı bilet; yerel ağ: kişiye özel giriş çerezi). UYAP'a istek atılmaz.
//
// Sol menüde "İş Listesi"; sayfa İcra Takip Süreçleri gibi pencere davranır: görev çubuğunda kendi
// düğmesi (kaynak 'isler'), küçült / öne getir, menüde başka sayfaya gidince küçülür, "UYAP'ta aç" ile
// açılan dosya kartı sayfanın üstünde açılır ve kapanınca kullanıcı buraya döner.
//
// Sekmeler: Bugün (gecikenler dahil) · Takvim · Tüm işler · Onay bekleyenler · Rutinler · Şablonlar.
// İş kartı sağda açılan paneldir (üst üste binmez); "Ertesi güne aktar", sonuç seçerek kapatma ve
// sonraki iş önerisi panelin içindedir. Renkler tema değişkenlerinden (--cu-t-*, --cu-vurgu*).
//
// Bildirim (plan L6, 16.11.2026): sayfa kapalıyken de `ozet` dakikada bir ofisten yoklanır (sekme gizliyken
// durur). Sol menüdeki "İş Listesi"nde rozet; sağ altta uyarı kartı: başkasının atadığı yeni iş, onaya düşen
// iş, zamanı gelen 🔔 hatırlatıcı (Tamam / 1 saat sonra). Hepsi ofis bilgisayarında kalır, buluta gitmez.
(function() {
    'use strict';
    const anaCerceve = window.__uyapAnaCerceve ? window.__uyapAnaCerceve() : window.top === window;
    if (!anaCerceve || window.__cuIsler) return;
    window.__cuIsler = true;

    const UC = '/__cu__/veri/is/';
    const ACIK_ANAHTAR = 'cuIslerAcik';
    const GUNLER = ['Pzt', 'Sal', 'Çar', 'Per', 'Cum', 'Cmt', 'Paz'];
    const AYLAR = ['Ocak', 'Şubat', 'Mart', 'Nisan', 'Mayıs', 'Haziran', 'Temmuz', 'Ağustos', 'Eylül', 'Ekim', 'Kasım', 'Aralık'];
    const DURUM_RENK = { acik: '#3f7fa3', devam: '#b8901f', onayda: '#7a5fb0', tamamlandi: '#4f8a57', iptal: '#8a877e' };

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
    function bildir(m) { if (window.__uyapBildirim) window.__uyapBildirim(m); }
    function gunMetni(iso) {
        const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(iso || '');
        return m ? m[3] + '.' + m[2] + '.' + m[1] : '';
    }
    function isoGun(t) { return t.getFullYear() + '-' + String(t.getMonth() + 1).padStart(2, '0') + '-' + String(t.getDate()).padStart(2, '0'); }
    function gunAl(iso) { const p = iso.split('-').map(Number); return new Date(p[0], p[1] - 1, p[2]); }
    function gunEkle(iso, n) { const t = gunAl(iso); t.setDate(t.getDate() + n); return isoGun(t); }
    // Ertesi iş günü (hafta sonu atlanır; resmî tatili sunucu bilmez burada, kullanıcı tarihi değiştirebilir)
    function ertesiIsGunu(iso) {
        let t = gunAl(iso); t.setDate(t.getDate() + 1);
        while (t.getDay() === 0 || t.getDay() === 6) t.setDate(t.getDate() + 1);
        return isoGun(t);
    }
    function secenek(deger, ad, secili) { const o = e('option', { value: deger, metin: ad }); if (String(secili) === String(deger)) o.selected = true; return o; }
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
            const h = new Error(j.kod === 'E-402' ? 'Büro veritabanı hazır değil (E-402). Ofis bilgisayarında program açık ve paylaşım başlamış olmalı.'
                : (j.error || 'İşlem yapılamadı.'));
            h.kod = j.kod;
            throw h;
        }
        return j.veri;
    }

    // ── Stil ──
    function stilKur() {
        if (document.getElementById('cu-isler-stil')) return;
        const css = [
            '#cu-isler{--i-zemin:var(--cu-t-zemin,#f3f4f7);--i-yuzey:var(--cu-t-yuzey,#fff);--i-serit:var(--cu-t-serit,#f5f6fa);',
            '--i-kenar:var(--cu-t-kenar,#dee2e6);--i-yazi:var(--cu-t-yazi,#212529);--i-vurgu:var(--cu-vurgu,#2d4596);',
            '--i-vurgu-koyu:var(--cu-vurgu-koyu,#1f3277);--i-vurgu-zemin:var(--cu-vurgu-zemin,#e8ecf8);',
            '--i-soluk:color-mix(in srgb,var(--i-yazi) 74%,var(--i-yuzey));--i-silik:color-mix(in srgb,var(--i-yazi) 64%,var(--i-yuzey));',
            'position:fixed;z-index:1400;top:var(--cu-nav-yuk,60px);right:0;bottom:var(--cu-cubuk-yuk,44px);left:0;',
            'background:var(--i-zemin);color:var(--i-yazi);font:14px/1.45 "Segoe UI",system-ui,sans-serif;display:flex;flex-direction:column;overflow:hidden}',
            // Güneş: vurgu sarı; vurgu rengindeki yazılar (seçili sekme, etiketler, bağlantılar) beyaz zeminde okunmuyordu
            // (2026-11-23 karşıtlık taraması). Yazı olarak temanın koyu vurgu-yazı tonu kullanılır.
            'html[data-cu-tema="gunes"] #cu-isler{--i-vurgu-koyu:var(--cu-vurgu-yazi,#6b4a00)}',
            'html[data-cu-tema="koyu"] #cu-isler{--i-zemin:#1d2130;--i-yuzey:#2a2f40;--i-serit:#262b3b;--i-kenar:#3a4055;--i-yazi:#dedede;',
            '--i-vurgu:#3a55b5;--i-vurgu-koyu:#8ea2e6;--i-vurgu-zemin:#2c3554}',
            '#cu-isler *{box-sizing:border-box}',
            '#cu-isler button,#cu-isler input,#cu-isler select,#cu-isler textarea{font:inherit;color:inherit}',
            '#cu-isler .i-bas{display:flex;align-items:center;gap:14px;padding:14px 22px 0;flex-wrap:wrap}',
            '#cu-isler .i-bas h1{font-size:19px;margin:0;font-weight:600}',
            '#cu-isler .i-durum{color:var(--i-soluk);font-size:12.5px}',
            '#cu-isler .i-bosluk{flex:1}',
            '#cu-isler .i-sekmeler{display:flex;gap:2px;padding:10px 22px 0;border-bottom:1px solid var(--i-kenar);overflow-x:auto}',
            '#cu-isler .i-sekmeler button{background:none;border:0;border-bottom:2px solid transparent;padding:8px 12px;color:var(--i-soluk);cursor:pointer;margin-bottom:-1px;white-space:nowrap}',
            '#cu-isler .i-sekmeler button.on{color:var(--i-vurgu-koyu);border-color:var(--i-vurgu);font-weight:600}',
            '#cu-isler .rozet{display:inline-block;min-width:18px;padding:0 6px;margin-left:5px;border-radius:999px;background:var(--i-vurgu);color:var(--cu-vurgu-uzeri,#fff);font-size:11px;font-weight:700;line-height:18px;text-align:center}',
            '#cu-isler .rozet.kirmizi{background:#b84444;color:#fff}',
            '#cu-isler .i-govde{flex:1;display:flex;min-height:0;position:relative}',
            '#cu-isler .i-ana{flex:1;overflow:auto;padding:16px 22px 22px;min-width:0}',
            '#cu-isler .i-arac{display:flex;gap:10px;align-items:center;margin-bottom:12px;flex-wrap:wrap}',
            '#cu-isler input[type=search],#cu-isler input[type=text],#cu-isler input[type=date],#cu-isler input[type=time],#cu-isler input[type=number],#cu-isler select,#cu-isler textarea{background:var(--i-yuzey);border:1px solid var(--i-kenar);border-radius:9px;padding:7px 10px;min-width:0}',
            '#cu-isler textarea{width:100%;min-height:64px;resize:vertical}',
            '#cu-isler input[type=datetime-local]{background:var(--i-yuzey);border:1px solid var(--i-kenar);border-radius:9px;padding:6px 10px;min-width:0;max-width:100%}',
            '#cu-isler input[type=search]{flex:1;max-width:420px}',
            '#cu-isler .i-onay{display:flex;align-items:center;gap:6px;color:var(--i-soluk);cursor:pointer;user-select:none}',
            '#cu-isler .i-d{border:1px solid var(--i-kenar);background:var(--i-yuzey);border-radius:9px;padding:7px 13px;cursor:pointer;white-space:nowrap}',
            '#cu-isler .i-d:hover{border-color:var(--i-vurgu)}',
            '#cu-isler .i-d.ana{background:var(--i-vurgu);border-color:var(--i-vurgu);color:var(--cu-vurgu-uzeri,#fff)}',
            '#cu-isler .i-d.kucuk{padding:4px 9px;font-size:12.5px;border-radius:8px}',
            '#cu-isler .i-d.tehlike{color:#b84444}',
            '#cu-isler .i-d[disabled]{opacity:.55;cursor:default}',
            '#cu-isler .bolum{font-size:11.5px;letter-spacing:.04em;color:var(--i-silik);text-transform:uppercase;margin:14px 0 8px;font-weight:600}',
            '#cu-isler .bolum:first-child{margin-top:0}',
            '#cu-isler .is{display:flex;gap:12px;align-items:flex-start;background:var(--i-yuzey);border:1px solid var(--i-kenar);border-radius:12px;padding:10px 13px;margin-bottom:7px;cursor:pointer}',
            '#cu-isler .is:hover{border-color:var(--i-vurgu)}',
            '#cu-isler .is.sec{border-color:var(--i-vurgu);box-shadow:0 0 0 2px var(--i-vurgu-zemin)}',
            '#cu-isler .is .ic{flex:1;min-width:0}',
            '#cu-isler .is .bs{font-weight:600}',
            '#cu-isler .is .alt,#cu-isler .alt{color:var(--i-soluk);font-size:12.5px}',
            '#cu-isler .is .sag{text-align:right;white-space:nowrap;font-size:12.5px;color:var(--i-soluk)}',
            '#cu-isler .oncelik{width:4px;align-self:stretch;border-radius:2px;background:var(--i-kenar)}',
            '#cu-isler .oncelik.p2{background:#b84444}#cu-isler .oncelik.p0{background:transparent}',
            '#cu-isler .chip{display:inline-block;padding:1px 8px;border-radius:999px;font-size:12px;font-weight:500;white-space:nowrap;margin:2px 4px 0 0;',
            'background:color-mix(in srgb,var(--r,#8a877e) 16%,var(--i-yuzey));color:color-mix(in srgb,var(--r,#8a877e) 80%,var(--i-yazi))}',
            '#cu-isler .gec{color:#b84444;font-weight:600}',
            '#cu-isler .e-suz{display:flex;gap:6px;flex-wrap:wrap;align-items:center;margin:-4px 0 12px}',
            '#cu-isler .e-cip{border:1px solid var(--i-kenar);background:var(--i-yuzey);border-radius:999px;padding:3px 11px;font-size:12.5px;cursor:pointer;white-space:nowrap;color:inherit}',
            '#cu-isler .e-cip.sec{background:color-mix(in srgb,var(--r,#8a877e) 22%,var(--i-yuzey));border-color:color-mix(in srgb,var(--r,#8a877e) 60%,var(--i-kenar));font-weight:600}',
            '#cu-isler .e-suz select{padding:3px 8px;font-size:12.5px;border-radius:999px}',
            '#cu-isler .i-bilgi{background:var(--i-yuzey);border:1px dashed var(--i-kenar);border-radius:12px;padding:22px;color:var(--i-soluk);text-align:center}',
            '#cu-isler .i-hata{background:color-mix(in srgb,#b84444 12%,var(--i-yuzey));color:color-mix(in srgb,#b84444 80%,var(--i-yazi));border-radius:10px;padding:9px 13px;margin-bottom:12px}',
            '#cu-isler .i-panel{width:480px;max-width:100%;background:var(--i-yuzey);border-left:1px solid var(--i-kenar);display:flex;flex-direction:column;box-shadow:-10px 0 24px rgba(0,0,0,.06)}',
            '#cu-isler .i-panel .p-bas{padding:14px 18px 10px;border-bottom:1px solid var(--i-kenar)}',
            '#cu-isler .i-panel h2{font-size:17px;margin:0;word-break:break-word}',
            '#cu-isler .i-panel .p-ic{flex:1;overflow:auto;padding:12px 18px 18px}',
            '#cu-isler .kutu{border:1px solid var(--i-kenar);border-radius:12px;padding:11px 13px;margin-bottom:10px}',
            '#cu-isler .kutu.vurgu{border-color:var(--i-vurgu);background:var(--i-vurgu-zemin)}',
            '#cu-isler .sat{display:flex;justify-content:space-between;gap:12px;font-size:13px;margin-top:5px;color:var(--i-soluk)}',
            '#cu-isler .sat b{color:var(--i-yazi);font-weight:500;text-align:right}',
            '#cu-isler .eylem{display:flex;gap:6px;flex-wrap:wrap;margin-top:9px;align-items:center}',
            '#cu-isler .form{display:grid;grid-template-columns:120px minmax(0,1fr);gap:8px 10px;align-items:center}',
            '#cu-isler .form label.b{color:var(--i-soluk);font-size:12.5px}',
            '#cu-isler .form .tam{grid-column:1/-1}',
            '#cu-isler .olay{border-left:2px solid var(--i-kenar);padding:0 0 10px 12px;margin-left:4px;position:relative;font-size:13px}',
            '#cu-isler .olay:before{content:"";position:absolute;left:-5px;top:5px;width:8px;height:8px;border-radius:50%;background:var(--i-vurgu)}',
            '#cu-isler .not{background:var(--i-serit);border-radius:10px;padding:8px 11px;margin-bottom:6px;white-space:pre-wrap;word-break:break-word}',
            '#cu-isler .ilerleme{height:5px;border-radius:3px;background:var(--i-kenar);overflow:hidden;margin-top:6px}',
            '#cu-isler .ilerleme i{display:block;height:100%;background:var(--i-vurgu)}',
            // Takvim
            '#cu-isler .tk{display:grid;grid-template-columns:repeat(7,minmax(0,1fr));gap:4px}',
            '#cu-isler .tk .gb{font-size:12px;color:var(--i-soluk);text-align:center;padding:2px 0}',
            '#cu-isler .tk .g{background:var(--i-yuzey);border:1px solid var(--i-kenar);border-radius:9px;min-height:96px;padding:4px 5px;overflow:hidden}',
            '#cu-isler .tk .g.disi{opacity:.5}#cu-isler .tk .g.tatil{background:var(--i-serit)}',
            '#cu-isler .tk .g.bugun{border-color:var(--i-vurgu);box-shadow:inset 0 0 0 1px var(--i-vurgu)}',
            '#cu-isler .tk .g .n{font-size:12px;font-weight:600;color:var(--i-soluk)}',
            '#cu-isler .tk .t{display:block;width:100%;text-align:left;border:0;border-radius:6px;padding:1px 5px;margin-top:2px;font-size:11.5px;cursor:pointer;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;',
            'background:color-mix(in srgb,var(--r,#3f7fa3) 16%,var(--i-yuzey));color:color-mix(in srgb,var(--r,#3f7fa3) 85%,var(--i-yazi))}',
            '#cu-isler .tk .t.sanal{cursor:default;opacity:.75;font-style:italic}',
            '#cu-isler table{width:100%;border-collapse:separate;border-spacing:0;background:var(--i-yuzey);border:1px solid var(--i-kenar);border-radius:12px;overflow:hidden}',
            '#cu-isler th{text-align:left;font-weight:600;font-size:12px;color:var(--i-soluk);background:var(--i-serit);padding:8px 12px}',
            '#cu-isler td{padding:8px 12px;border-top:1px solid var(--i-kenar);vertical-align:top}',
            '#cu-menu-isler .SidebarLabel{font-weight:600}',
            '#cu-menu-isler.on > li > a{background:var(--cu-vurgu-zemin,#e8ecf8)}',
            // Dar ekran
            'html.cu-dar #cu-isler .i-bas,html.cu-dar #cu-isler .i-sekmeler{padding-left:12px;padding-right:12px}',
            'html.cu-dar #cu-isler .i-ana{padding:12px}',
            'html.cu-dar #cu-isler .i-panel{position:absolute;inset:0;width:auto;border-left:0;z-index:2}',
            'html.cu-dar #cu-isler .tk .g{min-height:64px}',
            'html.cu-dar #cu-isler .form{grid-template-columns:1fr}',
        ].join('\n');
        document.head.appendChild(e('style', { id: 'cu-isler-stil', metin: css }));
    }

    // ── Bildirim stili: menü rozeti ve uyarı kartları (sayfa açılmadan da gerekir) ──
    function bildirimStilKur() {
        if (document.getElementById('cu-is-bildirim-stil')) return;
        const css = [
            '#cu-menu-isler .cu-is-rozet{display:inline-block;min-width:18px;padding:0 6px;margin-left:6px;border-radius:999px;background:var(--cu-vurgu,#2d4596);color:var(--cu-vurgu-uzeri,#fff);font-size:11px;font-weight:700;line-height:18px;text-align:center;vertical-align:middle}',
            '#cu-menu-isler .cu-is-rozet.kirmizi{background:#b84444;color:#fff}',
            '#cu-menu-isler .cu-is-nokta{display:inline-block;width:8px;height:8px;border-radius:50%;background:#e0912f;margin-left:5px;vertical-align:middle}',
            '#cu-is-uyarilar{position:fixed;right:14px;bottom:calc(var(--cu-cubuk-yuk,44px) + 12px);z-index:2147483000;display:flex;flex-direction:column;gap:8px;width:350px;max-width:calc(100vw - 28px);pointer-events:none}',
            '.cu-is-uyari{--u-yuzey:var(--cu-t-yuzey,#fff);--u-yazi:var(--cu-t-yazi,#212529);--u-kenar:var(--cu-t-kenar,#dee2e6);pointer-events:auto;background:var(--u-yuzey);color:var(--u-yazi);',
            'border:1px solid var(--u-kenar);border-left:4px solid var(--cu-vurgu,#2d4596);border-radius:12px;padding:10px 12px;box-shadow:0 10px 28px rgba(0,0,0,.18);font:13.5px/1.45 "Segoe UI",system-ui,sans-serif}',
            '.cu-is-uyari.hatirlatma{border-left-color:#c9962b}',
            'html[data-cu-tema="koyu"] .cu-is-uyari{--u-yuzey:#2a2f40;--u-yazi:#dedede;--u-kenar:#3a4055}',
            '.cu-is-uyari .u-bas{display:flex;align-items:center;gap:8px}.cu-is-uyari .u-bas b{flex:1;font-weight:600}',
            '.cu-is-uyari .u-x{border:0;background:none;color:inherit;opacity:.6;cursor:pointer;font-size:13px;padding:2px 4px}',
            '.cu-is-uyari .u-metin{margin-top:3px;word-break:break-word;color:color-mix(in srgb,var(--u-yazi) 82%,var(--u-yuzey))}',
            '.cu-is-uyari .u-eylem{display:flex;gap:6px;flex-wrap:wrap;margin-top:8px;justify-content:flex-end}',
            '.cu-is-uyari .u-d{border:1px solid var(--u-kenar);background:var(--u-yuzey);color:inherit;border-radius:8px;padding:4px 10px;font:inherit;font-size:12.5px;cursor:pointer}',
            '.cu-is-uyari .u-d.ana{background:var(--cu-vurgu,#2d4596);border-color:var(--cu-vurgu,#2d4596);color:var(--cu-vurgu-uzeri,#fff)}',
            '.cu-is-uyari .u-d[disabled]{opacity:.55;cursor:default}',
        ].join('\n');
        document.head.appendChild(e('style', { id: 'cu-is-bildirim-stil', metin: css }));
    }
    // Hatırlatma zamanı: "2026-11-16T09:30" (yerel) ↔ gösterim "Bugün 09:30" / "Yarın 09:00" / "18.11.2026 09:00".
    function yerelZaman(t) {
        return isoGun(t) + 'T' + String(t.getHours()).padStart(2, '0') + ':' + String(t.getMinutes()).padStart(2, '0');
    }
    function kisaZaman(z) {
        const m = /^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}:\d{2})/.exec(z || '');
        if (!m) return z || '';
        const gun = m[1] + '-' + m[2] + '-' + m[3], bugun = isoGun(new Date());
        return (gun === bugun ? 'Bugün' : gun === gunEkle(bugun, 1) ? 'Yarın' : m[3] + '.' + m[2] + '.' + m[1]) + ' ' + m[4];
    }
    function gecmisZaman(z) { return !!z && new Date(z).getTime() < Date.now() - 60000; }
    // Hazır seçenekler (1 saat sonra, bu akşam 17:00, yarın 09:00, son tarihte 09:00) + tarih/saat kutusu.
    // `sonTarih`: "YYYY-AA-GG" ya da onu veren işlev (formda tarih değişince).
    function hatirlatSecici(sonTarih) {
        const girdi = e('input', { type: 'datetime-local' });
        const cipler = e('div', { sinif: 'e-suz', stil: 'margin:0 0 6px' });
        function saatte(gun, ss, iso) {
            const t = iso ? gunAl(iso) : new Date();
            if (!iso) t.setDate(t.getDate() + gun);
            t.setHours(ss, 0, 0, 0);
            return t;
        }
        function ciz() {
            bos(cipler);
            const simdi = Date.now();
            const l = [['1 saat sonra', new Date(Math.ceil((simdi + 3600000) / 300000) * 300000)], ['Bu akşam 17:00', saatte(0, 17)], ['Yarın 09:00', saatte(1, 9)]];
            const st = typeof sonTarih === 'function' ? sonTarih() : sonTarih;
            if (st && st !== isoGun(new Date(simdi + 86400000))) l.push(['Son tarihte 09:00', saatte(0, 9, st)]);
            l.filter(function(x) { return x[1].getTime() > simdi + 60000; }).forEach(function(x) {
                const v = yerelZaman(x[1]);
                cipler.appendChild(e('button', { type: 'button', sinif: 'e-cip' + (girdi.value === v ? ' sec' : ''), metin: x[0],
                    onclick: function() { girdi.value = girdi.value === v ? '' : v; ciz(); } }));
            });
        }
        girdi.addEventListener('input', ciz);
        ciz();
        return { el: e('div', {}, [cipler, girdi]), deger: function() { return girdi.value; }, tazele: ciz };
    }

    // ── Durum ──
    const d = {
        sekme: 'bugun', ben: null, bugun: isoGun(new Date()), kisiler: [], sablonlar: [], sabitler: null, ozet: null,
        filtre: { ara: '', durum: 'acik', sorumlu: '', tur: '', etiket: '' }, takvimAy: null, herkes: false, etiketler: [],
        secili: null, panel: null, oneri: null, hata: '', kimlikYok: false,
        dosyaSuzgec: null,   // {dosya_no, birim_adi}: dosya kartından "İşleri göster"
        donus: null          // dosya kartı anahtarı: iş açılınca / vazgeçilince o karta dönülür
    };
    let kok = null, govde = null, araZaman = null, gorunum = 'kapali';
    const buradanAcilan = new Set();

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
    // Görünen ad hesabı ele vermiyorsa (ör. zeynel@savas → "savas") hesap adını ekle: listede kim olduğu anlaşılsın.
    function kisiEtiketle(l) {
        return (l || []).map(function(k) {
            const h = String(k.hesap || '').split('@')[0];
            if (!h || String(k.ad || '').toLocaleLowerCase('tr').indexOf(h.toLocaleLowerCase('tr')) >= 0) return k;
            return Object.assign({}, k, { ad: (k.ad || '') + ' (' + h + ')' });
        });
    }
    function kisiAdi(id) { const k = d.kisiler.find(function(x) { return x.id === id; }); return k ? k.ad : ''; }
    function adlar(liste, kod) { const x = (liste || []).find(function(y) { return y.kod === kod; }); return x ? x.ad : kod; }

    // ── Menü girişi ──
    function menuGirisi() {
        const yan = document.getElementById('av-sidebar-wrapper');
        if (!yan || document.getElementById('cu-menu-isler')) return;
        const ust = yan.querySelector('.sidebar-top-part') || yan.firstElementChild || yan;
        const ul = e('ul', { id: 'cu-menu-isler', sinif: 'menu-list' + (gorunum === 'acik' ? ' on' : '') }, [
            e('li', { sinif: 'SidebarLink' }, [
                e('a', { href: '#', title: 'Büronun işleri: bugün yapılacaklar, takvim, onay bekleyenler, rutinler',
                         onclick: function(ev) { ev.preventDefault(); ac(true); } },
                  [e('span', { sinif: 'SidebarLabel', metin: 'İş Listesi' })])
            ])
        ]);
        ust.insertBefore(ul, ust.firstChild);
        menuRozet();
    }
    // Rozet: bugünkü iş sayısı (gecikmiş varsa kırmızı); turuncu nokta: yeni iş / onay / hatırlatma bekliyor.
    function menuRozet() {
        const a = document.querySelector('#cu-menu-isler a');
        if (!a) return;
        bildirimStilKur();
        const o = d.ozet || {};
        const onay = (o.onay || 0) + (o.erteleme || 0), yeni = o.yeni || 0, hat = o.hatirlatma_sayi || 0;
        let r = a.querySelector('.cu-is-rozet'), n = a.querySelector('.cu-is-nokta');
        if (!r) { r = e('span', { sinif: 'cu-is-rozet' }); a.appendChild(r); }
        if (!n) { n = e('span', { sinif: 'cu-is-nokta' }); a.appendChild(n); }
        r.textContent = o.bugun ? String(o.bugun) : '';
        r.style.display = o.bugun ? '' : 'none';
        r.classList.toggle('kirmizi', !!o.geciken);
        n.style.display = yeni || onay || hat ? '' : 'none';
        const ozet = [o.bugun ? o.bugun + ' iş bugün' + (o.geciken ? ' (' + o.geciken + ' gecikmiş)' : '') : '', yeni ? yeni + ' yeni iş' : '',
            onay ? onay + ' onay bekliyor' : '', hat ? hat + ' hatırlatma' : ''].filter(Boolean).join(' · ');
        a.title = 'Büronun işleri: bugün yapılacaklar, takvim, onay bekleyenler, rutinler' + (ozet ? '\n' + ozet : '');
    }

    // ── Pencere ──
    function yerlestir() {
        if (!kok) return;
        const ic = document.getElementById('page-content-wrapper');
        let sol = 0;
        if (ic && !dar()) sol = Math.max(0, Math.round(ic.getBoundingClientRect().left));
        if (kok.style.left !== sol + 'px') kok.style.left = sol + 'px';
    }
    function ac(one) {
        stilKur();
        gorunum = 'acik';
        durumSakla();
        if (!kok) {
            kok = e('div', { id: 'cu-isler', role: 'region', 'aria-label': 'İş Listesi' });
            document.body.appendChild(kok);
            ciz();
            basla();
        }
        digerSayfalariIndir();
        kok.style.display = '';
        if (one && kartAcikMi() && window.__cuGorunurKartiKucult) window.__cuGorunurKartiKucult();
        const m = document.getElementById('cu-menu-isler');
        if (m) m.classList.add('on');
        yerlestir();
        gorevGuncelle(true);
    }
    // İş Listesi ile İcra Takip Süreçleri aynı alanı kaplayan iki ayrı pencere: aynı anda yalnız biri önde
    // durur. Biri açılınca öbürü görev çubuğuna iner (kullanıcı bildirimi 2026-11-23: İcra açıkken İş
    // Listesi'ne basınca İcra önde kalıyordu).
    const sayfalar = window.__cuSayfalar = window.__cuSayfalar || {};
    sayfalar['isler'] = function() { if (gorunum === 'acik') kucult(); };
    function digerSayfalariIndir() {
        Object.keys(sayfalar).forEach(function(k) { if (k !== 'isler') try { sayfalar[k](); } catch (x) {} });
    }
    function kucult() {
        if (gorunum === 'kapali') return;
        gorunum = 'kucuk';
        durumSakla();
        buradanAcilan.clear();
        if (kok) kok.style.display = 'none';
        const m = document.getElementById('cu-menu-isler');
        if (m) m.classList.remove('on');
        gorevGuncelle(true);
    }
    // Dosya kartından gelindiyse: sayfa görev çubuğuna iner, kart yeniden açılır (sekmesi ve belgesiyle).
    function kartaDon() {
        const k = d.donus;
        d.donus = null;
        if (!k || !window.__cuKartAc) return false;
        kucult();
        window.__cuKartAc(k);
        return true;
    }
    function kapat() {
        gorunum = 'kapali';
        durumSakla();
        buradanAcilan.clear();
        if (kok) kok.remove();
        kok = null; govde = null;
        const m = document.getElementById('cu-menu-isler');
        if (m) m.classList.remove('on');
        gorevGuncelle(true);
    }
    let gorevImza = null;
    function onde() { return gorunum === 'acik' && !kartAcikMi(); }
    function gorevTikla() { if (onde()) kucult(); else ac(true); }
    function gorevGuncelle(zorla) {
        if (!window.CU || !window.CU.gorev) return;
        const on = onde();
        const say = d.ozet ? (d.ozet.bugun || 0) : 0;
        const imza = gorunum + '|' + on + '|' + say;
        if (!zorla && imza === gorevImza) return;
        gorevImza = imza;
        if (gorunum === 'kapali') { window.CU.gorev.ayarla([], 'isler'); return; }
        window.CU.gorev.ayarla([{
            kimlik: 'isler', ad: 'İş Listesi', ikon: '✅', on: on, kucuk: !on, rozet: say ? String(say) : '',
            tikla: gorevTikla, kapat: kapat,
            menu: function() { return [[[onde() ? 'Küçült' : 'Öne getir', gorevTikla]]]; },
        }], 'isler');
    }

    // ── Çizim ──
    function ciz() {
        if (!kok) return;
        bos(kok);
        kok.appendChild(e('div', { sinif: 'i-bas' }, [
            e('h1', { metin: 'İş Listesi' }), e('div', { sinif: 'i-durum', id: 'cu-isler-durum' }), e('div', { sinif: 'i-bosluk' }),
            e('button', { sinif: 'i-d ana', metin: '+ Yeni iş', onclick: function() { yeniIs({}); } }),
            e('button', { sinif: 'i-d', metin: '–', title: 'Görev çubuğuna küçült (kaldığınız yer korunur)', 'aria-label': 'Küçült',
                          stil: 'min-width:38px;font-weight:700', onclick: kucult }),
            e('button', { sinif: 'i-d', metin: 'Kapat', title: 'Sayfayı kapat (görev çubuğundan da çıkar)', onclick: kapat })
        ]));
        kok.appendChild(sekmeCiz());
        govde = e('div', { sinif: 'i-govde' });
        kok.appendChild(govde);
        durumYaz();
    }
    function sekmeCiz() {
        const o = d.ozet || {};
        const sekmeler = [['bugun', 'Bugün', o.bugun, o.geciken ? 'kirmizi' : ''], ['takvim', 'Takvim'], ['tum', 'Tüm işler'],
            ['onay', 'Onay bekleyenler', (o.onay || 0) + (o.erteleme || 0)], ['rutin', 'Rutinler'], ['sablon', 'Şablonlar']];
        return e('div', { sinif: 'i-sekmeler', role: 'tablist' }, sekmeler.map(function(s) {
            return e('button', { sinif: d.sekme === s[0] ? 'on' : '', role: 'tab',
                                 onclick: function() { d.sekme = s[0]; d.panel = null; d.secili = null; ust(); yukle(); } },
                     [s[1], s[2] ? e('span', { sinif: 'rozet ' + (s[3] || ''), metin: String(s[2]) }) : null]);
        }));
    }
    // Sekmeler + durum yazısı yenilenir; liste ve panel yerinde kalır.
    function ust() {
        if (!kok) return;
        const eski = kok.querySelector('.i-sekmeler');
        if (eski) eski.replaceWith(sekmeCiz());
        durumYaz();
    }
    function durumYaz() {
        const el = document.getElementById('cu-isler-durum');
        if (!el) return;
        const o = d.ozet;
        el.textContent = (d.ben ? d.ben.ad + ' · ' : '') + (o ? o.bugun + ' iş bugün' + (o.geciken ? ' (' + o.geciken + ' gecikmiş)' : '')
            + (o.onay ? ' · ' + o.onay + ' onay bekliyor' : '') : '');
    }
    function hataGoster(x) {
        d.hata = x.message;
        const ana = e('div', { sinif: 'i-ana' }, [e('div', { sinif: 'i-hata', metin: x.message })]);
        if (x.kod === 'KIMLIK') ana.appendChild(e('a', { sinif: 'i-d ana', href: '/__uyap_lan__/giris?hedef=' + encodeURIComponent(location.pathname), metin: 'Kendi hesabımla giriş yap' }));
        else ana.appendChild(e('button', { sinif: 'i-d', metin: 'Yeniden dene', onclick: function() { d.ben ? yukle() : basla(); } }));
        bos(govde).appendChild(ana);
    }

    async function basla() {
        try {
            const b = await istek('ben');
            d.ben = b.ben; d.bugun = b.bugun; d.kisiler = kisiEtiketle(b.kisiler); d.sablonlar = b.sablonlar; d.sabitler = b.sabitler; d.ozet = b.ozet;
            d.etiketler = b.etiketler || [];
            d.filtre.sorumlu = '';
            yoklamaDur = false;
            ciz();
            yukle();
            gorevGuncelle(true);
            menuRozet();
            uyarilariIsle(d.ozet);
        } catch (x) { if (govde) hataGoster(x); }
    }
    function ozetTazele() { return yokla(true); }

    // ── Bildirim (plan L6) ──
    // Yoklama: dakikada bir (hata olunca 2, 4 … en çok 5 dk), sekme gizliyken yok; sekmeye dönünce hemen.
    // Kimlik yoksa (yerel ağdan girişsiz) durur; sayfa açılıp giriş yapılınca yeniden başlar.
    const UYARILAN = 'cuIsUyarilan', ONAY_TABAN = 'cuIsOnayTaban';
    let yoklamaSon = 0, yoklamaHata = 0, yoklamaDur = false, yoklamaSuruyor = false;
    function depoAl(k, varsayilan) {
        try { const v = JSON.parse(localStorage.getItem(k)); return v === null || v === undefined ? varsayilan : v; } catch (x) { return varsayilan; }
    }
    function depoYaz(k, v) { try { localStorage.setItem(k, JSON.stringify(v)); } catch (x) {} }
    async function yokla(zorla) {
        if (yoklamaSuruyor || (!zorla && (yoklamaDur || document.visibilityState === 'hidden'))) return;
        yoklamaSuruyor = true;
        yoklamaSon = Date.now();
        try {
            d.ozet = await istek('ozet');
            yoklamaHata = 0;
            yoklamaDur = false;
        } catch (x) {
            yoklamaHata++;
            if (x.kod === 'KIMLIK') yoklamaDur = true;
            return;
        } finally { yoklamaSuruyor = false; }
        ust(); gorevGuncelle(true); menuRozet(); uyarilariIsle(d.ozet);
    }
    function yoklamaAraligi() { return yoklamaHata ? Math.min(300000, 60000 * Math.pow(2, yoklamaHata)) : 60000; }

    // Uyarı kartları: sağ altta, en çok 4; kullanıcı kapatana ya da bir düğmeye basana kadar durur.
    const acikUyarilar = new Map();
    const susturulan = new Set();     // ✕ ile kapatılan hatırlatma: bu sayfada yeniden çıkmaz ("Tamam" denmedi)
    function uyariKapat(anahtar, elle) {
        const x = acikUyarilar.get(anahtar);
        if (x) { x.remove(); acikUyarilar.delete(anahtar); }
        if (elle) susturulan.add(anahtar);
    }
    function uyariGoster(anahtar, baslik, metin, dugmeler, sinif) {
        bildirimStilKur();
        uyariKapat(anahtar);
        let kutu = document.getElementById('cu-is-uyarilar');
        if (!kutu) { kutu = e('div', { id: 'cu-is-uyarilar', role: 'status', 'aria-live': 'polite' }); document.body.appendChild(kutu); }
        const kart = e('div', { sinif: 'cu-is-uyari' + (sinif ? ' ' + sinif : '') }, [
            e('div', { sinif: 'u-bas' }, [e('b', { metin: baslik }),
                e('button', { sinif: 'u-x', title: 'Kapat', 'aria-label': 'Kapat', metin: '✕', onclick: function() { uyariKapat(anahtar, true); } })]),
            e('div', { sinif: 'u-metin', metin: metin }),
            // düğme: [ad, işlev, ana mı, basınca kart kalsın mı]
            e('div', { sinif: 'u-eylem' }, dugmeler.filter(Boolean).map(function(b) {
                return e('button', { sinif: 'u-d' + (b[2] ? ' ana' : ''), metin: b[0], onclick: async function(ev) {
                    const t = ev.currentTarget;
                    t.disabled = true;
                    try { await b[1](); if (b[3]) t.disabled = false; else uyariKapat(anahtar); } catch (x) { t.disabled = false; bildir(x.message); }
                } });
            }))
        ]);
        kutu.appendChild(kart);
        acikUyarilar.set(anahtar, kart);
        while (acikUyarilar.size > 4) uyariKapat(acikUyarilar.keys().next().value);
    }
    function sekmeyeGit(sekme) {
        ac(true);
        hazirOlunca(function() { d.sekme = sekme; d.panel = null; d.secili = null; d.dosyaSuzgec = null; ust(); yukle(); });
    }
    function uyarilariIsle(o) {
        if (!o) return;
        // Yeni iş: her (iş, atama olayı) bir kez uyarılır (sekmeler ve sayfa yenilemeleri arasında da).
        const uyarilan = depoAl(UYARILAN, []);
        const yeniler = (o.yeni_isler || []).filter(function(y) { return uyarilan.indexOf(y.id + ':' + y.olay) < 0; });
        if (yeniler.length) {
            yeniler.forEach(function(y) { uyarilan.push(y.id + ':' + y.olay); });
            depoYaz(UYARILAN, uyarilan.slice(-300));
            if (yeniler.length === 1) {
                const y = yeniler[0];
                uyariGoster('y' + y.id, 'Yeni iş', (y.atayan ? y.atayan + ' size bir iş atadı: ' : 'Size bir iş atandı: ') + y.baslik,
                    [['Aç', function() { window.__cuIsGoster(y.id); }, true]]);
            } else {
                const toplam = Math.max(o.yeni || 0, yeniler.length);
                uyariGoster('yeni', 'Yeni işler', toplam + ' yeni işiniz var: ' + yeniler.slice(0, 3).map(function(y) { return y.baslik; }).join(' · ') + (toplam > 3 ? ' …' : ''),
                    [['Tüm işler', function() { sekmeyeGit('tum'); }, true]]);
            }
        }
        // Onay: sayı arttıysa (ilk yoklamada yalnız taban alınır).
        const onay = (o.onay || 0) + (o.erteleme || 0);
        const taban = depoAl(ONAY_TABAN, null);
        if (typeof taban === 'number' && onay > taban) {
            uyariGoster('onay', 'Onay bekliyor', 'Onayınızı bekleyen ' + onay + ' iş / erteleme talebi var.', [['Göster', function() { sekmeyeGit('onay'); }, true]]);
        }
        if (taban !== onay) depoYaz(ONAY_TABAN, onay);
        // Hatırlatma: "Tamam" denene kadar her yoklamada gelir; başka sekmede Tamam denince kart kalkar.
        const gelen = o.hatirlatma || [];
        const anahtarlar = gelen.map(function(h) { return 'h' + h.id; });
        Array.from(acikUyarilar.keys()).forEach(function(k) { if (/^h\d+$/.test(k) && anahtarlar.indexOf(k) < 0) uyariKapat(k); });
        gelen.forEach(function(h) {
            const k = 'h' + h.id;
            if (acikUyarilar.has(k) || susturulan.has(k)) return;
            uyariGoster(k, '🔔 Hatırlatma · ' + kisaZaman(h.zaman), h.baslik + (h.notu && h.notu !== h.baslik ? ' — ' + h.notu : ''), [
                h.gorev_id ? ['İşi aç', function() { window.__cuIsGoster(h.gorev_id); }, false, true] : null,   // kart kalır
                ['1 saat sonra', async function() { await istek('hatirlatici_ertele', { id: h.id, dakika: 60 }); bildir('Bir saat sonra yeniden hatırlatılacak.'); yokla(true); }],
                ['Tamam', async function() { await istek('hatirlatici_tamam', { id: h.id }); yokla(true); }, true]
            ], 'hatirlatma');
        });
    }

    async function yukle() {
        if (!govde || !d.ben) return;
        d.hata = '';
        try {
            if (d.sekme === 'bugun') listeCiz(await istek('liste?gorunum=bugun'), 'bugun');
            else if (d.sekme === 'onay') listeCiz(await istek('liste?gorunum=onay'), 'onay');
            else if (d.sekme === 'tum') {
                const f = d.filtre;
                const durum = f.durum === 'acik' ? 'acik,devam,onayda' : f.durum === 'kapali' ? 'tamamlandi,iptal' : '';
                const q = new URLSearchParams({ gorunum: 'tum', ara: f.ara, durum: durum, sorumlu: f.sorumlu, tur: f.tur, sinir: '300' });
                if (d.dosyaSuzgec) { q.set('dosya_no', d.dosyaSuzgec.dosya_no); if (d.dosyaSuzgec.birim_adi) q.set('birim_adi', d.dosyaSuzgec.birim_adi); }
                if (f.etiket) q.set('etiket', f.etiket);
                listeCiz(await istek('liste?' + q.toString()), 'tum');
            } else if (d.sekme === 'takvim') takvimCiz();
            else if (d.sekme === 'rutin') rutinCiz(await istek('tekrarlar'));
            else sablonCiz(await istek('sablonlar'));
        } catch (x) { hataGoster(x); }
    }

    // ── Etiketler: 'is-' anahtarlılar (UYAP İşi, Adliye İşi, Müvekkil Görüşme, Borçlu Araması) öne çıkar ──
    function onEtiketler() { return d.etiketler.filter(function(t) { return /^is-/.test(t.anahtar || ''); }); }
    function digerEtiketler() { return d.etiketler.filter(function(t) { return !/^is-/.test(t.anahtar || ''); }); }
    function etiketBul(id) { return d.etiketler.find(function(t) { return t.id === id; }); }
    function etiketCipleri(ids) {
        return (ids || []).map(etiketBul).filter(Boolean).map(function(t) { return e('span', { sinif: 'chip', stil: '--r:' + (t.renk || '#6f7d95'), metin: t.ad }); });
    }
    // Süzgeç satırı: Hepsi + öne çıkan etiketler + "Diğer etiket" seçimi.
    function etiketSuzgeci() {
        const f = d.filtre;
        const sec = function(id) { f.etiket = String(f.etiket) === String(id) ? '' : String(id); yukle(); };
        const cipler = [e('button', { sinif: 'e-cip' + (f.etiket ? '' : ' sec'), metin: 'Hepsi', onclick: function() { f.etiket = ''; yukle(); } })];
        onEtiketler().forEach(function(t) {
            cipler.push(e('button', { sinif: 'e-cip' + (String(f.etiket) === String(t.id) ? ' sec' : ''), stil: '--r:' + (t.renk || '#6f7d95'), metin: t.ad, onclick: function() { sec(t.id); } }));
        });
        const diger = digerEtiketler();
        if (diger.length) {
            const digerSecili = diger.some(function(t) { return String(t.id) === String(f.etiket); });
            const s0 = e('select', { onchange: function() { f.etiket = s0.value; yukle(); } },
                [secenek('', 'Diğer etiket…', digerSecili ? f.etiket : '')].concat(diger.map(function(t) { return secenek(t.id, t.ad, digerSecili ? f.etiket : ''); })));
            cipler.push(s0);
        }
        return e('div', { sinif: 'e-suz' }, cipler);
    }
    // Formda etiket seçimi (çoklu): öne çıkanlar çip, diğerleri "+ diğer" ile açılır. .deger() → [id]
    function etiketSecici(secili) {
        const set = new Set((secili || []).map(Number));
        const kutu = e('div', { sinif: 'e-suz', stil: 'margin:0' });
        let digerAcik = (secili || []).some(function(id) { const t = etiketBul(id); return t && !/^is-/.test(t.anahtar || ''); });
        function ciz() {
            bos(kutu);
            const goster = onEtiketler().concat(digerAcik ? digerEtiketler() : []);
            goster.forEach(function(t) {
                kutu.appendChild(e('button', { type: 'button', sinif: 'e-cip' + (set.has(t.id) ? ' sec' : ''), stil: '--r:' + (t.renk || '#6f7d95'), metin: t.ad,
                    onclick: function() { if (set.has(t.id)) set.delete(t.id); else set.add(t.id); ciz(); } }));
            });
            if (!digerAcik && digerEtiketler().length) kutu.appendChild(e('button', { type: 'button', sinif: 'e-cip', metin: '+ diğer', onclick: function() { digerAcik = true; ciz(); } }));
        }
        ciz();
        kutu.deger = function() { return Array.from(set); };
        kutu.ayarla = function(ids) { set.clear(); (ids || []).forEach(function(x) { set.add(Number(x)); }); ciz(); };
        return kutu;
    }

    function isSatiri(g) {
        const gec = g.gecikti ? e('span', { sinif: 'gec', metin: g.gecikme_gun + ' gün gecikti' }) : null;
        const chipler = [];
        if (g.durum !== 'acik') chipler.push(e('span', { sinif: 'chip', stil: '--r:' + (DURUM_RENK[g.durum] || '#8a877e'), metin: g.durum_adi }));
        if (g.tur && g.tur !== 'is') chipler.push(e('span', { sinif: 'chip', metin: adlar(d.sabitler && d.sabitler.turler, g.tur) }));
        if (g.erteleme_bekliyor) chipler.push(e('span', { sinif: 'chip', stil: '--r:#c06a2f', metin: 'Erteleme bekliyor' }));
        if (g.onay_gerekir && g.durum !== 'onayda' && g.durum !== 'tamamlandi') chipler.push(e('span', { sinif: 'chip', metin: 'Onaylı iş' }));
        if (g.gizli) chipler.push(e('span', { sinif: 'chip', metin: 'Gizli' }));
        if (g.hatirlatma) chipler.push(e('span', { sinif: 'chip', stil: '--r:#b8901f', title: 'Size hatırlatılacak', metin: '🔔 ' + kisaZaman(g.hatirlatma) }));
        etiketCipleri(g.etiketler).forEach(function(c) { chipler.push(c); });
        const dosya = [g.birim_adi, g.dosya_no].filter(Boolean).join(' ');
        return e('div', { sinif: 'is' + (d.secili === g.id ? ' sec' : ''), onclick: function() { isAc(g.id); } }, [
            e('div', { sinif: 'oncelik p' + g.oncelik, title: ['Düşük', 'Normal', 'Yüksek'][g.oncelik] + ' öncelik' }),
            e('div', { sinif: 'ic' }, [
                e('div', { sinif: 'bs', metin: g.baslik }),
                dosya || g.taraf_adi ? e('div', { sinif: 'alt', metin: [dosya, g.taraf_adi].filter(Boolean).join(' · ') }) : null,
                chipler.length ? e('div', {}, chipler) : null,
                g.alt_sayisi ? e('div', { sinif: 'alt', metin: g.alt_biten + '/' + g.alt_sayisi + ' alt iş' }) : null
            ]),
            e('div', { sinif: 'sag' }, [e('div', { metin: gunMetni(g.son_tarih) + (g.son_saat ? ' ' + g.son_saat : '') }),
                e('div', { metin: g.sorumlu_adi || '—' }), gec])
        ]);
    }

    function listeCiz(liste, tur) {
        if (d.filtre.etiket) { const et = Number(d.filtre.etiket); liste = liste.filter(function(g) { return (g.etiketler || []).indexOf(et) >= 0; }); }
        bos(govde);
        const ana = e('div', { sinif: 'i-ana' });
        govde.appendChild(ana);
        if (tur === 'tum' && d.dosyaSuzgec) {
            const ds = d.dosyaSuzgec;
            ana.appendChild(e('div', { sinif: 'kutu vurgu', stil: 'display:flex;gap:10px;align-items:center;flex-wrap:wrap' }, [
                e('b', { metin: 'Dosyanın işleri: ' + [ds.birim_adi, ds.dosya_no].filter(Boolean).join(' ') }), e('div', { sinif: 'i-bosluk' }),
                d.donus ? e('button', { sinif: 'i-d kucuk', metin: 'Karta dön', onclick: kartaDon }) : null,
                e('button', { sinif: 'i-d kucuk', metin: 'Süzgeci kaldır', onclick: function() { d.dosyaSuzgec = null; d.filtre.ara = ''; yukle(); } })]));
        }
        if (tur === 'tum') {
            const f = d.filtre;
            const ara = e('input', { type: 'search', placeholder: 'Başlık, dosya no, birim, taraf, açıklama ara', value: f.ara,
                oninput: function() { clearTimeout(araZaman); araZaman = setTimeout(function() { f.ara = ara.value; yukle(); }, 300); } });
            const durum = e('select', { onchange: function() { f.durum = durum.value; yukle(); } },
                [secenek('acik', 'Açık işler', f.durum), secenek('kapali', 'Kapanmış', f.durum), secenek('', 'Hepsi', f.durum)]);
            const kisi = e('select', { onchange: function() { f.sorumlu = kisi.value; yukle(); } },
                [secenek('', 'Herkes', f.sorumlu)].concat(d.kisiler.map(function(k) { return secenek(k.id, k.ad, f.sorumlu); })));
            const turSec = e('select', { onchange: function() { f.tur = turSec.value; yukle(); } },
                [secenek('', 'Her tür', f.tur)].concat(((d.sabitler && d.sabitler.turler) || []).map(function(t) { return secenek(t.kod, t.ad, f.tur); })));
            ana.appendChild(e('div', { sinif: 'i-arac' }, [ara, durum, kisi, turSec]));
        }
        if ((tur === 'tum' || tur === 'bugun' || tur === 'onay') && d.etiketler.length) ana.appendChild(etiketSuzgeci());
        if (tur === 'tum') {
            const f = d.filtre, ara = ana.querySelector('input[type=search]');
            if (f.ara && ara && document.activeElement !== ara) setTimeout(function() { ara.focus(); ara.setSelectionRange(ara.value.length, ara.value.length); }, 0);
        }
        if (!liste.length) {
            ana.appendChild(e('div', { sinif: 'i-bilgi', metin: d.filtre.etiket ? 'Bu etikette iş yok.' : tur === 'bugun' ? 'Bugün için işiniz yok. 🎉' : tur === 'onay' ? 'Onay bekleyen iş yok.' : 'Bu ölçütlere uyan iş yok.' }));
        } else if (tur === 'bugun') {
            const gecen = liste.filter(function(g) { return g.gecikti; }), bugun = liste.filter(function(g) { return !g.gecikti; });
            if (gecen.length) { ana.appendChild(e('div', { sinif: 'bolum', metin: 'Gecikmiş (' + gecen.length + ')' })); gecen.forEach(function(g) { ana.appendChild(isSatiri(g)); }); }
            if (bugun.length) { ana.appendChild(e('div', { sinif: 'bolum', metin: 'Bugün (' + bugun.length + ')' })); bugun.forEach(function(g) { ana.appendChild(isSatiri(g)); }); }
        } else {
            liste.forEach(function(g) { ana.appendChild(isSatiri(g)); });
            if (liste.length >= 300) ana.appendChild(e('div', { sinif: 'alt', metin: 'İlk 300 iş gösteriliyor; aramayı daraltın.' }));
        }
        panelCiz();
    }

    // ── Takvim ──
    async function takvimCiz() {
        if (!d.takvimAy) { const t = gunAl(d.bugun); d.takvimAy = new Date(t.getFullYear(), t.getMonth(), 1); }
        const ay = d.takvimAy;
        const ilk = new Date(ay); ilk.setDate(1 - ((ay.getDay() + 6) % 7));
        const son = new Date(ilk); son.setDate(ilk.getDate() + 41);
        const v = await istek('takvim?' + new URLSearchParams({ ilk: isoGun(ilk), son: isoGun(son), herkes: d.herkes ? '1' : '' }).toString());
        bos(govde);
        const ana = e('div', { sinif: 'i-ana' });
        govde.appendChild(ana);
        const git = function(n) { return function() { d.takvimAy = new Date(ay.getFullYear(), ay.getMonth() + n, 1); takvimCiz().catch(hataGoster); }; };
        const herkes = e('input', { type: 'checkbox' }); herkes.checked = d.herkes;
        herkes.addEventListener('change', function() { d.herkes = herkes.checked; takvimCiz().catch(hataGoster); });
        ana.appendChild(e('div', { sinif: 'i-arac' }, [
            e('button', { sinif: 'i-d kucuk', metin: '‹', onclick: git(-1) }),
            e('b', { metin: AYLAR[ay.getMonth()] + ' ' + ay.getFullYear(), stil: 'min-width:120px;text-align:center' }),
            e('button', { sinif: 'i-d kucuk', metin: '›', onclick: git(1) }),
            e('button', { sinif: 'i-d kucuk', metin: 'Bugün', onclick: function() { d.takvimAy = null; takvimCiz().catch(hataGoster); } }),
            e('div', { sinif: 'i-bosluk' }), e('label', { sinif: 'i-onay' }, [herkes, 'Bütün büro'])
        ]));
        const gunler = {};
        v.isler.concat(v.rutinler).forEach(function(g) { (gunler[g.son_tarih] = gunler[g.son_tarih] || []).push(g); });
        const tatil = new Set(v.tatiller);
        const tk = e('div', { sinif: 'tk' }, GUNLER.map(function(g) { return e('div', { sinif: 'gb', metin: g }); }));
        for (let i = 0; i < 42; i++) {
            const t = new Date(ilk); t.setDate(ilk.getDate() + i);
            const iso = isoGun(t);
            const hucre = e('div', { sinif: 'g' + (t.getMonth() !== ay.getMonth() ? ' disi' : '') + (tatil.has(iso) || t.getDay() === 0 || t.getDay() === 6 ? ' tatil' : '') + (iso === d.bugun ? ' bugun' : ''),
                                     ondblclick: function() { yeniIs({ son_tarih: iso }); }, title: 'Çift tıklayın: bu güne iş ekle' },
                [e('div', { sinif: 'n', metin: String(t.getDate()) })]);
            (gunler[iso] || []).sort(function(a, b) { return (a.son_saat || '99').localeCompare(b.son_saat || '99'); }).forEach(function(g) {
                const renk = g.sanal ? '#8a877e' : g.gecikti ? '#b84444' : (DURUM_RENK[g.durum] || '#3f7fa3');
                hucre.appendChild(e('button', { sinif: 't' + (g.sanal ? ' sanal' : ''), stil: '--r:' + renk,
                    title: g.baslik + (g.sorumlu_adi ? ' — ' + g.sorumlu_adi : '') + (g.sanal ? ' (rutin; günü gelince açılır)' : ''),
                    metin: (g.son_saat ? g.son_saat + ' ' : '') + g.baslik,
                    onclick: g.sanal ? null : function(ev) { ev.stopPropagation(); isAc(g.id); } }));
            });
            tk.appendChild(hucre);
        }
        ana.appendChild(tk);
        panelCiz();
    }

    // ── Rutinler ──
    function rutinCiz(liste) {
        bos(govde);
        const ana = e('div', { sinif: 'i-ana' });
        govde.appendChild(ana);
        ana.appendChild(e('div', { sinif: 'alt', stil: 'margin-bottom:10px', metin: 'Tekrar eden işler: günü gelince listeye kendiliğinden düşer. Bitmeyen rutin gecikmiş olarak kalır, her gün elle aktarmanız gerekmez.' }));
        ana.appendChild(e('button', { sinif: 'i-d ana', metin: '+ Yeni rutin', stil: 'margin-bottom:12px', onclick: rutinFormu }));
        if (!liste.length) ana.appendChild(e('div', { sinif: 'i-bilgi', metin: 'Henüz rutin yok.' }));
        else ana.appendChild(e('table', {}, [
            e('thead', {}, [e('tr', {}, ['Rutin', 'Ne zaman', 'Sorumlu', ''].map(function(b) { return e('th', { metin: b }); }))]),
            e('tbody', {}, liste.map(function(r) {
                const ne = r.kural_adi + (r.haftanin_gunleri && r.haftanin_gunleri.length ? ' (' + r.haftanin_gunleri.map(function(g) { return GUNLER[g - 1]; }).join(', ') + ')' : '')
                    + (r.ayin_gunu ? ' (ayın ' + r.ayin_gunu + '. günü)' : '') + (r.saat ? ' · ' + r.saat : '');
                return e('tr', {}, [e('td', {}, [e('b', { metin: r.baslik }), r.aktif === false ? e('span', { sinif: 'alt', metin: ' (durduruldu)' }) : null]),
                    e('td', { metin: ne }), e('td', { metin: r.sorumlu_adi }),
                    e('td', { stil: 'text-align:right' }, [e('button', { sinif: 'i-d kucuk', metin: r.aktif === false ? 'Başlat' : 'Durdur', onclick: async function() {
                        try { await istek('tekrar_durdur', { id: r.id, aktif: r.aktif === false }); yukle(); } catch (x) { bildir(x.message); }
                    } })])]);
            }))
        ]));
        panelCiz();
    }
    function rutinFormu() {
        const baslik = e('input', { type: 'text', placeholder: 'Ör. UYAP tebligat kontrolü' });
        const kural = e('select', {}, ((d.sabitler && d.sabitler.tekrar_kurallari) || []).map(function(k) { return secenek(k.kod, k.ad, 'is_gunu'); }));
        const gunKutulari = GUNLER.map(function(g, i) { const c = e('input', { type: 'checkbox', value: String(i + 1) }); return [c, e('label', { sinif: 'i-onay' }, [c, g])]; });
        const gunSatiri = e('div', { stil: 'display:flex;gap:8px;flex-wrap:wrap' }, gunKutulari.map(function(x) { return x[1]; }));
        const ayinGunu = e('input', { type: 'number', min: '1', max: '31', placeholder: '1–31', stil: 'width:90px' });
        const saat = e('input', { type: 'time' });
        const sorumlu = e('select', {}, d.kisiler.map(function(k) { return secenek(k.id, k.ad, d.ben.id); }));
        const baslangic = e('input', { type: 'date', value: d.bugun });
        const gizli = e('input', { type: 'checkbox' });
        const gunSat = e('label', { sinif: 'b', metin: 'Günler' }), aySat = e('label', { sinif: 'b', metin: 'Ayın günü' });
        function goster() {
            const k = kural.value;
            gunSat.style.display = gunSatiri.style.display = k === 'haftalik' ? '' : 'none';
            aySat.style.display = ayinGunu.style.display = k === 'aylik' ? '' : 'none';
        }
        kural.addEventListener('change', goster);
        panelAc('Yeni rutin', [e('div', { sinif: 'form' }, [
            e('label', { sinif: 'b', metin: 'Başlık' }), baslik, e('label', { sinif: 'b', metin: 'Tekrar' }), kural,
            gunSat, gunSatiri, aySat, ayinGunu, e('label', { sinif: 'b', metin: 'Saat' }), saat,
            e('label', { sinif: 'b', metin: 'Sorumlu' }), sorumlu, e('label', { sinif: 'b', metin: 'Başlangıç' }), baslangic,
            e('span'), e('label', { sinif: 'i-onay' }, [gizli, 'Gizli (yalnız ben görürüm)']),
            e('div', { sinif: 'tam eylem' }, [e('button', { sinif: 'i-d ana', metin: 'Kaydet', onclick: async function() {
                try {
                    await istek('tekrar', { baslik: baslik.value, kural: kural.value, saat: saat.value, sorumlu_id: Number(sorumlu.value),
                        baslangic: baslangic.value, gizli: gizli.checked, ayin_gunu: ayinGunu.value ? Number(ayinGunu.value) : null,
                        haftanin_gunleri: gunKutulari.filter(function(x) { return x[0].checked; }).map(function(x) { return Number(x[0].value); }) });
                    bildir('Rutin eklendi.'); d.panel = null; yukle(); ozetTazele();
                } catch (x) { bildir(x.message); }
            } })])
        ])]);
        goster();
        baslik.focus();
    }

    // ── Şablonlar ──
    function sablonCiz(liste) {
        d.sablonlar = liste;
        bos(govde);
        const ana = e('div', { sinif: 'i-ana' });
        govde.appendChild(ana);
        ana.appendChild(e('div', { sinif: 'alt', stil: 'margin-bottom:10px', metin: 'Şablon işin başlığını, süresini ve onay gerekip gerekmediğini belirler; kapatırken seçilen sonuca göre sonraki işi önerir.' }));
        if (d.ben.yonetici) ana.appendChild(e('button', { sinif: 'i-d ana', metin: '+ Yeni şablon', stil: 'margin-bottom:12px', onclick: function() { sablonFormu(null); } }));
        ana.appendChild(e('table', {}, [
            e('thead', {}, [e('tr', {}, ['Şablon', 'Süre', 'Onay', 'Sonraki işler', ''].map(function(b) { return e('th', { metin: b }); }))]),
            e('tbody', {}, liste.filter(function(s) { return s.aktif !== false; }).map(function(s) {
                const sn = s.sonraki || {};
                const sonraki = Object.keys(sn).map(function(k) { const x = sn[k]; const h = liste.find(function(y) { return y.id === x.sonraki_sablon_id; });
                    return (x.ad || adlar(d.sabitler && d.sabitler.sonuclar, k)) + ' → ' + (h ? h.baslik : 'aynı iş') + (x.sure_deger != null ? ' (' + x.sure_deger + ' g.)' : ''); }).join(' · ');
                return e('tr', {}, [e('td', {}, [e('b', { metin: s.baslik }), s.aciklama ? e('div', { sinif: 'alt', metin: s.aciklama }) : null]),
                    e('td', { metin: s.sure_metni || (s.sure_deger + ' ' + adlar(d.sabitler && d.sabitler.sure_turleri, s.sure_turu)) }),
                    e('td', { metin: s.onay_gerekir ? 'Gerekir' : '—' }), e('td', { sinif: 'alt', metin: sonraki }),
                    e('td', { stil: 'text-align:right;white-space:nowrap' }, [
                        e('button', { sinif: 'i-d kucuk', metin: 'İş aç', onclick: function() { yeniIs({ sablon_id: s.id }); } }),
                        d.ben.yonetici ? e('button', { sinif: 'i-d kucuk', metin: 'Düzenle', stil: 'margin-left:4px', onclick: function() { sablonFormu(s); } }) : null])]);
            }))
        ]));
        panelCiz();
    }
    function sablonFormu(s) {
        s = s || {};
        const baslik = e('input', { type: 'text', value: s.baslik || '' });
        const aciklama = e('textarea', {}, [s.aciklama || '']);
        const kontrol = e('input', { type: 'text', value: s.kontrol || '', placeholder: 'Ör. Mazbata döndü mü?' });
        const sureTuru = e('select', {}, ((d.sabitler && d.sabitler.sure_turleri) || []).map(function(t) { return secenek(t.kod, t.ad, s.sure_turu || 'is_gunu'); }));
        const sure = e('input', { type: 'number', min: '0', value: s.sure_deger != null ? String(s.sure_deger) : '1', stil: 'width:90px' });
        const onay = e('input', { type: 'checkbox' }); onay.checked = !!s.onay_gerekir;
        const desen = e('input', { type: 'text', value: s.evrak_deseni || '', placeholder: 'Belge adında geçen sözcükler (İş ata önerisi)' });
        // Sonuca göre sonraki iş (B-6/7/19): sonuç adı, sonraki şablon ("aynı iş" = yinelenir), iş günü.
        const dallar = ((d.sabitler && d.sabitler.sonuclar) || []).map(function(r) {
            const x = (s.sonraki || {})[r.kod];
            const secili = x ? (x.sonraki_sablon_id || 'ayni') : 'yok';
            const ad = e('input', { type: 'text', value: x ? x.ad : '', placeholder: r.ad, stil: 'width:120px' });
            const hedef = e('select', { stil: 'flex:1;min-width:0' }, [secenek('yok', '— sonraki iş yok —', secili), secenek('ayni', '↻ Aynı iş yinelenir', secili)]
                .concat(d.sablonlar.filter(function(y) { return y.aktif !== false && y.id !== s.id; }).map(function(y) { return secenek(y.id, y.baslik, secili); })));
            const gun = e('input', { type: 'number', min: '0', placeholder: 'gün', value: x && x.sure_deger != null ? String(x.sure_deger) : '', stil: 'width:70px',
                                     title: 'Kaç iş günü sonra (boş: sonraki şablonun kendi süresi)' });
            return { kod: r.kod, el: e('div', { stil: 'display:flex;gap:6px;align-items:center;margin-bottom:5px' }, [e('span', { sinif: 'alt', stil: 'width:110px', metin: r.ad }), ad, hedef, gun]),
                     deger: function() {
                         if (hedef.value === 'yok') return null;
                         return { ad: ad.value, sonraki_sablon_id: hedef.value === 'ayni' ? null : Number(hedef.value),
                                  sure_turu: gun.value !== '' ? 'is_gunu' : '', sure_deger: gun.value !== '' ? Number(gun.value) : null };
                     } };
        });
        panelAc(s.id ? 'Şablonu düzenle' : 'Yeni şablon', [e('div', { sinif: 'form' }, [
            e('label', { sinif: 'b', metin: 'Başlık' }), baslik, e('label', { sinif: 'b', metin: 'Açıklama' }), aciklama,
            e('label', { sinif: 'b', metin: 'Kontrol sorusu' }), kontrol,
            e('label', { sinif: 'b', metin: 'Süre' }), e('div', { stil: 'display:flex;gap:8px' }, [sure, sureTuru]),
            e('span'), e('label', { sinif: 'i-onay' }, [onay, 'Onay gerekir']),
            e('label', { sinif: 'b', metin: 'Belge deseni' }), desen,
            e('div', { sinif: 'tam bolum', metin: 'Kapatırken sonuca göre sonraki iş' }),
            e('div', { sinif: 'tam' }, dallar.map(function(x) { return x.el; })),
            e('div', { sinif: 'tam eylem' }, [e('button', { sinif: 'i-d ana', metin: 'Kaydet', onclick: async function() {
                const sonraki = {};
                dallar.forEach(function(x) { const v = x.deger(); if (v) sonraki[x.kod] = v; });
                try {
                    // Formda olmayan alanlar (taraf, eylem, olay türü, süreç aşaması, etiketler) olduğu gibi gönderilir: güncelleme silmesin.
                    await istek('sablon', { id: s.id || null, baslik: baslik.value, aciklama: aciklama.value, kontrol: kontrol.value, sure_turu: sureTuru.value,
                        sure_deger: Number(sure.value || 0), onay_gerekir: onay.checked, evrak_deseni: desen.value, tur: s.tur || 'is', oncelik: s.oncelik != null ? s.oncelik : 1,
                        taraf: s.taraf || '', eylem: s.eylem || '', olay_turu: s.olay_turu || '', surec_asama_id: s.surec_asama_id || null, aktif: true,
                        etiketler: s.etiketler || [], sonraki: sonraki });
                    bildir('Şablon kaydedildi.'); d.panel = null; yukle();
                } catch (x) { bildir(x.message); }
            } })])
        ])]);
    }

    // ── Sağ panel ──
    function panelAc(baslik, icerik) {
        d.secili = null;
        d.panel = { baslik: baslik, icerik: icerik };
        panelCiz();
    }
    function panelCiz() {
        if (!govde) return;
        const eski = govde.querySelector('.i-panel');
        if (eski) eski.remove();
        if (!d.panel) return;
        const kapatDugmesi = e('button', { sinif: 'i-d kucuk', metin: '✕', title: 'Paneli kapat', 'aria-label': 'Paneli kapat',
            onclick: function() { d.panel = null; d.secili = null; d.oneri = null; panelCiz(); govde.querySelectorAll('.is.sec').forEach(function(x) { x.classList.remove('sec'); }); kartaDon(); } });
        const p = d.panel;
        const panel = e('div', { sinif: 'i-panel' }, [
            e('div', { sinif: 'p-bas' }, [e('div', { stil: 'display:flex;justify-content:space-between;gap:10px;align-items:flex-start' },
                [e('h2', { metin: p.baslik }), e('div', { stil: 'display:flex;gap:6px' }, (p.dugmeler || []).concat([kapatDugmesi]))]),
                p.alt ? e('div', { sinif: 'alt', metin: p.alt }) : null, p.chipler ? e('div', {}, p.chipler) : null]),
            e('div', { sinif: 'p-ic' }, p.icerik)
        ]);
        govde.appendChild(panel);
    }

    function yeniIs(on) {
        if (on.sablon_adi && !on.sablon_id) {
            const ad = String(on.sablon_adi).toLocaleLowerCase('tr');
            const s0 = d.sablonlar.find(function(x) { return x.aktif !== false && x.baslik.toLocaleLowerCase('tr') === ad; });
            if (s0) on.sablon_id = s0.id;
        }
        const sablon = e('select', {}, [secenek('', '— Şablonsuz —', on.sablon_id || '')].concat(d.sablonlar.filter(function(s) { return s.aktif !== false; })
            .map(function(s) { return secenek(s.id, s.baslik, on.sablon_id || ''); })));
        const baslik = e('input', { type: 'text', value: on.baslik || '' });
        const aciklama = e('textarea', {}, [on.aciklama || '']);
        const tur = e('select', {}, ((d.sabitler && d.sabitler.turler) || []).filter(function(t) { return t.kod !== 'rutin'; }).map(function(t) { return secenek(t.kod, t.ad, on.tur || 'is'); }));
        const tarih = e('input', { type: 'date', value: on.son_tarih || '' });
        const saat = e('input', { type: 'time', value: on.son_saat || '' });
        const yer = e('input', { type: 'text', placeholder: 'Duruşma / keşif / haciz yeri' });
        const sorumlu = e('select', {}, d.kisiler.map(function(k) { return secenek(k.id, k.ad, on.sorumlu_id || d.ben.id); }));
        const onay = e('input', { type: 'checkbox' }); onay.checked = !!on.onay_gerekir;
        const oncelik = e('select', {}, [secenek(0, 'Düşük', 1), secenek(1, 'Normal', 1), secenek(2, 'Yüksek', 1)]);
        const gizli = e('input', { type: 'checkbox' });
        const etiket = etiketSecici(on.etiketler || []);
        const hatirlat = hatirlatSecici(function() { return tarih.value; });
        tarih.addEventListener('change', hatirlat.tazele);
        const ipucu = e('div', { sinif: 'alt tam' });
        // Şablonu olmayan bir iş yazılınca: formdaki başlık/açıklama/tür/onay/öncelik/süre/etiketler büroya şablon olur.
        const sablonYap = e('button', { type: 'button', sinif: 'i-d kucuk', stil: 'display:none;justify-self:start', metin: '☆ Bu işi şablon olarak kaydet',
            title: 'Büronun şablonlarına eklenir; düzenlemek için Şablonlar sekmesi', onclick: async function() {
                const ad = baslik.value.trim();
                if (!ad) { bildir('Şablon için başlık yazın.'); return; }
                const gun = tarih.value ? Math.max(0, Math.round((gunAl(tarih.value) - gunAl(d.bugun)) / 86400000)) : 1;
                try {
                    const v = await istek('sablon', { baslik: ad, aciklama: aciklama.value, tur: tur.value, onay_gerekir: onay.checked,
                        oncelik: Number(oncelik.value), sure_turu: 'takvim_gunu', sure_deger: gun, etiketler: etiket.deger() });
                    d.sablonlar = await istek('sablonlar');
                    sablon.appendChild(secenek(v.id, ad, v.id));
                    sablon.value = String(v.id);
                    baslik.dataset.sablondan = '1';
                    sablonYapGoster();
                    bildir('Şablon kaydedildi: ' + ad);
                } catch (x) { bildir(x.message); }
            } });
        function sablonYapGoster() {
            const ad = baslik.value.trim().toLocaleLowerCase('tr');
            const var_ = d.sablonlar.some(function(x) { return x.baslik.toLocaleLowerCase('tr') === ad; });
            sablonYap.style.display = ad && !var_ ? '' : 'none';
        }
        baslik.addEventListener('input', sablonYapGoster);
        function sablonaUy() {
            const s = d.sablonlar.find(function(x) { return String(x.id) === sablon.value; });
            if (!s) { ipucu.textContent = 'Son tarih boşsa bugün sayılmaz; tarih seçin.'; return; }
            if (!baslik.value || baslik.dataset.sablondan) { baslik.value = s.baslik; baslik.dataset.sablondan = '1'; }
            onay.checked = s.onay_gerekir;
            if ((s.etiketler || []).length && !etiket.deger().length) etiket.ayarla(s.etiketler);
            sablonYapGoster();
            ipucu.textContent = 'Son tarihi boş bırakırsanız şablona göre hesaplanır' + (s.sure_metni ? ' (' + s.sure_metni + ')' : '') + '.';
        }
        sablon.addEventListener('change', sablonaUy);
        baslik.addEventListener('input', function() { delete baslik.dataset.sablondan; });
        const dosyaBilgi = on.dosya_no ? e('div', { sinif: 'kutu tam' }, [e('b', { metin: [on.birim_adi, on.dosya_no].filter(Boolean).join(' ') }),
            on.evrak_adi ? e('div', { sinif: 'alt', metin: on.evrak_adi }) : null]) : null;
        panelAc('Yeni iş', [e('div', { sinif: 'form' }, [
            dosyaBilgi,
            e('label', { sinif: 'b', metin: 'Şablon' }), sablon, e('label', { sinif: 'b', metin: 'Başlık' }), baslik,
            e('span'), sablonYap,
            e('label', { sinif: 'b', metin: 'Etiket' }), etiket,
            e('label', { sinif: 'b', metin: 'Açıklama' }), aciklama, e('label', { sinif: 'b', metin: 'Tür' }), tur,
            e('label', { sinif: 'b', metin: 'Son tarih' }), e('div', { stil: 'display:flex;gap:8px' }, [tarih, saat]), ipucu,
            e('label', { sinif: 'b', metin: 'Yer' }), yer,
            e('label', { sinif: 'b', metin: 'Sorumlu' }), sorumlu, e('label', { sinif: 'b', metin: 'Öncelik' }), oncelik,
            e('label', { sinif: 'b', metin: 'Bana hatırlat' }), hatirlat.el,
            e('span'), e('label', { sinif: 'i-onay' }, [onay, 'Onay gerekir (tamamlanınca onaya gider)']),
            e('span'), e('label', { sinif: 'i-onay' }, [gizli, 'Gizli (yalnız ben ve sorumlu görür)']),
            e('div', { sinif: 'tam eylem' }, [e('button', { sinif: 'i-d ana', metin: 'İşi aç', onclick: async function() {
                if (!sablon.value && !tarih.value) { bildir('Son tarih seçin.'); return; }
                const hz = hatirlat.deger();
                if (gecmisZaman(hz)) { bildir('Hatırlatma zamanı geçmişte olamaz.'); return; }
                try {
                    const v = await istek('ac', Object.assign({}, on.dosya || {}, {
                        baslik: baslik.value, aciklama: aciklama.value, tur: tur.value, son_tarih: tarih.value, son_saat: saat.value, yer: yer.value,
                        sorumlu_id: Number(sorumlu.value), onay_gerekir: onay.checked, oncelik: Number(oncelik.value), gizli: gizli.checked,
                        sablon_id: sablon.value ? Number(sablon.value) : null, ust_gorev_id: on.ust_gorev_id || null, etiketler: etiket.deger() }));
                    // Hatırlatıcı ayrı istekte kurulur; kurulamazsa iş yine açılmış kalır.
                    let hHata = '';
                    if (hz) { try { await istek('hatirlatici', { gorev_id: v.id, zaman: hz }); } catch (x) { hHata = x.message; } }
                    bildir((onay.checked ? 'İş açıldı; tamamlanınca onaya gidecek.' : 'İş açıldı.')
                        + (hHata ? ' Hatırlatıcı kurulamadı: ' + hHata : hz ? ' Hatırlatıcı: ' + kisaZaman(hz) : ''));
                    yukle(); ozetTazele();
                    if (kartaDon()) return;
                    if (on.ust_gorev_id) isAc(on.ust_gorev_id); else isAc(v.id);
                } catch (x) { bildir(x.message); }
            } })])
        ])]);
        sablonaUy();
        baslik.focus();
    }

    // ── Ekler (18.11.2026): dosya ofis bilgisayarına (büro veritabanı + ekler klasörü) yüklenir, buluta gitmez.
    // İndirme fetch + blob ile: telefonda düz bağlantı Service Worker'ı atlayıp tünele girmiyordu (toplu indirmedeki ders).
    const EK_EN_BUYUK = 25 * 1024 * 1024;
    function boyutMetni(n) { return n >= 1048576 ? (n / 1048576).toFixed(1) + ' MB' : Math.max(1, Math.round(n / 1024)) + ' KB'; }
    function ad64(s) {
        const b = new TextEncoder().encode(s);
        let ikili = '';
        b.forEach(function(x) { ikili += String.fromCharCode(x); });
        return btoa(ikili).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
    }
    async function ekYukle(gorevId, dosya) {
        if (dosya.size > EK_EN_BUYUK) throw new Error(dosya.name + ': dosya çok büyük (en çok 25 MB).');
        const q = new URLSearchParams({ gorev: String(gorevId), ad64: ad64(dosya.name), mime: dosya.type || '' });
        let r;
        try {
            r = await fetch(UC + 'ek_yukle?' + q.toString(), { method: 'POST', cache: 'no-store', credentials: 'same-origin',
                headers: { 'Content-Type': 'application/octet-stream' }, body: dosya });
        } catch (x) { throw new Error('Ofis bilgisayarına ulaşılamadı.'); }
        let j = null;
        try { j = await r.json(); } catch (x) {}
        if (!j || !j.ok) throw new Error((j && j.error) || ('Yüklenemedi (' + r.status + ').'));
    }
    async function ekIndir(ek) {
        let r;
        try { r = await fetch(UC + 'ek_indir?id=' + ek.id, { cache: 'no-store', credentials: 'same-origin' }); }
        catch (x) { throw new Error('Ofis bilgisayarına ulaşılamadı.'); }
        if (!r.ok) {
            let j = null;
            try { j = await r.json(); } catch (x) {}
            throw new Error((j && j.error) || ('İndirilemedi (' + r.status + ').'));
        }
        const u = URL.createObjectURL(await r.blob());
        const a = document.createElement('a');
        a.href = u; a.download = ek.ad;
        a.setAttribute('data-uyap-internal', '1');
        document.body.appendChild(a); a.click(); a.remove();
        setTimeout(function() { URL.revokeObjectURL(u); }, 60000);
    }
    function ekKutusu(g, y) {
        const kutu = e('div', { sinif: 'kutu' }, [e('div', { sinif: 'bolum', stil: 'margin-top:0', metin: 'Ekler' + ((g.ekler || []).length ? ' (' + g.ekler.length + ')' : '') })]);
        const benId = d.ben && d.ben.id;
        (g.ekler || []).forEach(function(ek) {
            const silebilir = y.yonetebilir || (benId != null && ek.yukleyen_id === benId);
            const ad = e('button', { sinif: 'i-d kucuk', metin: '⬇ ' + ek.ad, title: 'İndir', stil: 'max-width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap', onclick: async function() {
                try { await ekIndir(ek); } catch (x) { bildir(x.message); }
            } });
            kutu.appendChild(e('div', { sinif: 'sat' }, [
                e('span', { stil: 'min-width:0;flex:1' }, [ad]),
                e('b', {}, [e('span', { sinif: 'alt', metin: boyutMetni(ek.boyut || 0) + (ek.yukleyen ? ' · ' + ek.yukleyen : '') }),
                    silebilir ? e('button', { sinif: 'i-d kucuk', metin: '✕', title: 'Eki sil', stil: 'margin-left:6px', onclick: async function() {
                        if (!window.confirm('"' + ek.ad + '" silinsin mi? Geri alınamaz.')) return;
                        try { await istek('ek_sil', { id: ek.id }); isAc(g.id); } catch (x) { bildir(x.message); }
                    } }) : null])
            ]));
        });
        const sec = e('input', { type: 'file' });
        sec.multiple = true;
        sec.style.display = 'none';
        const dugme = e('button', { sinif: 'i-d kucuk', metin: '+ Dosya ekle', onclick: function() { sec.click(); } });
        sec.addEventListener('change', async function() {
            const l = Array.prototype.slice.call(sec.files || []);
            if (!l.length) return;
            dugme.disabled = true;
            dugme.textContent = 'Yükleniyor…';
            let hata = '';
            for (const f of l) { try { await ekYukle(g.id, f); } catch (x) { hata = x.message; } }
            if (hata) bildir(hata); else bildir(l.length > 1 ? l.length + ' dosya eklendi.' : 'Dosya eklendi.');
            isAc(g.id);
        });
        kutu.appendChild(sec);
        kutu.appendChild(e('div', { sinif: 'eylem' }, [dugme]));
        kutu.appendChild(e('div', { sinif: 'alt', metin: 'Dosyalar ofis bilgisayarında saklanır (en çok 25 MB).' }));
        return kutu;
    }

    // ── Belgeler (24.11.2026): Armonia Yazı'da yazılıp büro veritabanına kaydedilen belgeler (belge_veri.rs).
    // Aç → editör (enjekte/yazi.js); "Yeni belge yaz" → editör bu işin dosyasına bağlı açılır, ilk kayıtta işe eklenir.
    function belgeKutusu(g, y) {
        const l = g.belgeler || [];
        const kutu = e('div', { sinif: 'kutu' }, [e('div', { sinif: 'bolum', stil: 'margin-top:0', metin: 'Belgeler' + (l.length ? ' (' + l.length + ')' : '') })]);
        const editor = !!window.__cuYaziBelgeAc;
        l.forEach(function(b) {
            const ust = ['sürüm ' + b.son_surum, b.guncelleme, b.imzali ? 'e-imzalı' : '', b.gonderim ? 'UYAP\'a gönderildi ' + b.gonderim.zaman : ''].filter(Boolean).join(' · ');
            kutu.appendChild(e('div', { sinif: 'sat' }, [
                e('span', { stil: 'min-width:0;flex:1' }, [e('button', { sinif: 'i-d kucuk', metin: '✎ ' + b.ad, title: editor ? 'Armonia Yazı\'da aç' : 'Editör bu sayfada yok',
                    disabled: !editor, stil: 'max-width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap',
                    onclick: function() { window.__cuYaziBelgeAc(b.id); } })]),
                e('b', {}, [e('span', { sinif: 'alt', metin: ust }),
                    y.calisabilir ? e('button', { sinif: 'i-d kucuk', metin: '✕', title: 'Belgeyi bu işten çıkar (belge silinmez)', stil: 'margin-left:6px', onclick: async function() {
                        try { await istek('belge_isten_cikar', { belge: b.id, gorev: g.id }); isAc(g.id); } catch (x) { bildir(x.message); }
                    } }) : null])
            ]));
        });
        if (y.calisabilir && editor) {
            const eylem = e('div', { sinif: 'eylem' }, [
                e('button', { sinif: 'i-d kucuk', metin: '+ Yeni belge yaz', title: 'Armonia Yazı\'da bu iş için belge yaz; ilk kayıtta işe eklenir', onclick: function() {
                    window.__cuYaziYeni({ buroDosyaId: g.dosya_id, birimAdi: g.birim_adi, dosyaNo: g.dosya_no, gorevId: g.id, kart: g.uyap_karti });
                } }),
                e('button', { sinif: 'i-d kucuk', metin: '+ Kayıtlı belge ekle', onclick: function() { belgeSecici(g, kutu); } })
            ]);
            kutu.appendChild(eylem);
        }
        if (g.durum === 'onayda' && y.onaylayabilir && l.length) kutu.appendChild(e('div', { sinif: 'alt', metin: 'Onaylamadan önce belgeleri açıp inceleyin; düzeltme gerekiyorsa "Geri çevir" ile not yazın.' }));
        return kutu;
    }
    // Kayıtlı belge seç: işin dosyasının belgeleri ya da kişinin belgeleri, aramayla.
    function belgeSecici(g, kutu) {
        if (kutu.querySelector('.i-belge-sec')) return;
        const ara = e('input', { type: 'text', placeholder: 'Belge adı, dosya no ya da birimle arayın', stil: 'width:100%;margin-top:6px' });
        const liste = e('div', { sinif: 'i-belge-sec' });
        let zaman = 0, sira = 0;
        async function yukle() {
            const n = ++sira;
            const q = new URLSearchParams();
            if (ara.value.trim()) q.set('ara', ara.value.trim()); else if (g.dosya_id) q.set('dosya', g.dosya_id);
            try {
                const s = await istek('belge_liste?' + q.toString());
                if (n !== sira) return;
                bos(liste);
                const ekli = new Set((g.belgeler || []).map(function(b) { return b.id; }));
                const uygun = s.filter(function(b) { return !ekli.has(b.id); });
                if (!uygun.length) liste.appendChild(e('div', { sinif: 'alt', metin: 'Eklenecek belge bulunamadı.' }));
                uygun.forEach(function(b) {
                    liste.appendChild(e('div', { sinif: 'sat' }, [e('span', { metin: b.ad }), e('b', {}, [e('span', { sinif: 'alt', metin: [b.birim_adi, b.dosya_no].filter(Boolean).join(' ') + ' · sürüm ' + b.son_surum }),
                        e('button', { sinif: 'i-d kucuk ana', metin: 'Ekle', stil: 'margin-left:6px', onclick: async function() {
                            try { await istek('belge_ise_ekle', { belge: b.id, gorev: g.id }); bildir('Belge eklendi.'); isAc(g.id); } catch (x) { bildir(x.message); }
                        } })])]));
                });
            } catch (x) { bos(liste); liste.appendChild(e('div', { sinif: 'alt', metin: x.message })); }
        }
        ara.addEventListener('input', function() { clearTimeout(zaman); zaman = setTimeout(yukle, 250); });
        kutu.appendChild(ara);
        kutu.appendChild(liste);
        yukle();
        ara.focus();
    }

    async function isAc(id) {
        try {
            const g = await istek('gorev?id=' + id);
            d.secili = id;
            isPaneli(g);
        } catch (x) { bildir(x.message); }
    }
    async function durumYap(g, durum, sonuc, notu) {
        try {
            const v = await istek('durum', { id: g.id, durum: durum, sonuc: sonuc || '', not: notu || '' });
            bildir(v.durum === 'onayda' ? 'Onaya gönderildi.' : v.durum === 'tamamlandi' ? 'İş tamamlandı.' : 'Durum güncellendi.');
            d.oneri = v.oneri ? Object.assign({ onceki: g.id }, v.oneri) : null;
            yukle(); ozetTazele(); isAc(g.id);
        } catch (x) { bildir(x.message); }
    }

    function isPaneli(g) {
        const y = g.yetki || {};
        const ic = [];
        // Sonraki iş önerisi (kapatınca hemen gelir; tarih değiştirilebilir)
        if (d.oneri && d.oneri.onceki === g.id) {
            const o = d.oneri;
            const t = e('input', { type: 'date', value: o.son_tarih || '' });
            const s = e('select', {}, d.kisiler.map(function(k) { return secenek(k.id, k.ad, o.sorumlu_id || d.ben.id); }));
            ic.push(e('div', { sinif: 'kutu vurgu' }, [e('div', { metin: 'Sonraki iş önerisi' + (o.sonuc_adi ? ' (' + o.sonuc_adi + ')' : '') + ':' }),
                e('b', { metin: o.baslik }), o.sure_metni ? e('div', { sinif: 'alt', metin: o.sure_metni }) : null,
                e('div', { sinif: 'eylem' }, [t, s, e('button', { sinif: 'i-d ana kucuk', metin: 'Aç', onclick: async function() {
                    if (!t.value) { bildir('Tarih seçin.'); return; }
                    try {
                        const v = await istek('oneriden_ac', { onceki: o.onceki, sablon_id: o.sablon_id, son_tarih: t.value, sorumlu_id: Number(s.value) });
                        d.oneri = null; bildir('Sonraki iş açıldı.'); yukle(); ozetTazele(); isAc(v.id);
                    } catch (x) { bildir(x.message); }
                } }), e('button', { sinif: 'i-d kucuk', metin: 'Gerek yok', onclick: function() { d.oneri = null; isPaneli(g); } })])]));
        }
        // Bilgi
        const dosya = [g.birim_adi, g.dosya_no].filter(Boolean).join(' ');
        ic.push(e('div', { sinif: 'kutu' }, [
            e('div', { sinif: 'sat' }, [e('span', { metin: 'Son tarih' }), e('b', { sinif: g.gecikti ? 'gec' : null, metin: gunMetni(g.son_tarih) + (g.son_saat ? ' ' + g.son_saat : '') + (g.gecikti ? ' · ' + g.gecikme_gun + ' gün gecikti' : '') })]),
            e('div', { sinif: 'sat' }, [e('span', { metin: 'Sorumlu' }), e('b', { metin: g.sorumlu_adi || '—' })]),
            e('div', { sinif: 'sat' }, [e('span', { metin: 'Açan' }), e('b', { metin: (g.olusturan_adi || '—') + (g.olusturulma ? ' · ' + gunMetni(g.olusturulma) : '') })]),
            dosya ? e('div', { sinif: 'sat' }, [e('span', { metin: 'Dosya' }), e('b', { metin: dosya })]) : null,
            g.taraf_adi ? e('div', { sinif: 'sat' }, [e('span', { metin: 'Taraf' }), e('b', { metin: g.taraf_adi })]) : null,
            g.evrak_adi ? e('div', { sinif: 'sat' }, [e('span', { metin: 'Belge' }), e('b', { metin: g.evrak_adi })]) : null,
            g.barkod ? e('div', { sinif: 'sat' }, [e('span', { metin: 'Barkod' }), e('b', { metin: g.barkod })]) : null,
            g.yer ? e('div', { sinif: 'sat' }, [e('span', { metin: 'Yer' }), e('b', { metin: g.yer })]) : null,
            g.surec_asama_adi ? e('div', { sinif: 'sat' }, [e('span', { metin: 'Süreç aşaması' }), e('b', { metin: g.surec_asama_adi })]) : null,
            g.sonuc ? e('div', { sinif: 'sat' }, [e('span', { metin: 'Sonuç' }), e('b', { metin: adlar(d.sabitler && d.sabitler.sonuclar, g.sonuc) })]) : null,
            g.aciklama ? e('div', { stil: 'margin-top:8px;white-space:pre-wrap;word-break:break-word', metin: g.aciklama }) : null
        ]));
        // Durum eylemleri
        const acik = ['acik', 'devam', 'onayda'].indexOf(g.durum) >= 0;
        const eylem = [];
        if ((g.durum === 'acik' || g.durum === 'devam') && (y.calisabilir || y.onaylayabilir)) {
            if (g.durum === 'acik') eylem.push(e('button', { sinif: 'i-d kucuk', metin: 'Başladım', onclick: function() { durumYap(g, 'devam'); } }));
            const kapatAdi = g.onay_gerekir && !y.onaylayabilir && !g.ust_gorev_id ? 'Onaya gönder' : 'Tamamla';
            if (g.sonuclar && g.sonuclar.length) {
                g.sonuclar.forEach(function(s) {
                    eylem.push(e('button', { sinif: 'i-d ana kucuk', metin: (s.ad || adlar(d.sabitler && d.sabitler.sonuclar, s.sonuc)), title: kapatAdi + (s.sonraki ? ' · sonraki: ' + s.sonraki : ''),
                        onclick: function() { durumYap(g, 'tamamla', s.sonuc); } }));
                });
            } else eylem.push(e('button', { sinif: 'i-d ana kucuk', metin: kapatAdi, onclick: function() { durumYap(g, 'tamamla'); } }));
        }
        if (g.durum === 'onayda') {
            // Hatırlatma: işi sunan / sorumlu, başkası onaylayacaksa (yönetici olsa da: kendisi onaylamayı beklemiyor)
            const onaylayicilar = (g.kisiler || []).filter(function(k) { return k.rol === 'onaylayici'; });
            const benOnaylarim = onaylayicilar.some(function(k) { return d.ben && k.id === d.ben.id; });
            const hatirlatabilir = y.calisabilir && !benOnaylarim && (!y.onaylayabilir || onaylayicilar.length > 0);
            if (hatirlatabilir) {
                eylem.push(e('button', { sinif: 'i-d ana kucuk', metin: 'Hatırlatma gönder', title: "Onaylayana Çoklu UYAP'ta hemen uyarı çıkar (aynı kişiye saatte bir)",
                    onclick: async function(ev) {
                        const b = ev.currentTarget;
                        const n = prompt2('Hatırlatmaya not ekleyin (isteğe bağlı)');
                        b.disabled = true;
                        try {
                            const v = await istek('onay_hatirlat', { gorev: g.id, notu: n });
                            bildir('Hatırlatma gönderildi: ' + v.kime.join(', ') + '.');
                            isAc(g.id);
                        } catch (x) { b.disabled = false; bildir(x.message); }
                    } }));
            }
            if (y.onaylayabilir) {
                eylem.push(e('button', { sinif: 'i-d' + (hatirlatabilir ? '' : ' ana') + ' kucuk', metin: 'Onayla', onclick: function() { durumYap(g, 'tamamla', g.sonuc); } }));
                eylem.push(e('button', { sinif: 'i-d kucuk', metin: hatirlatabilir ? 'Onaydan geri çek' : 'Geri çevir', onclick: function() {
                    if (hatirlatabilir) { durumYap(g, 'acik'); return; }
                    const n = prompt2('Geri çevirme nedeni (isteğe bağlı)'); durumYap(g, 'acik', '', n); } }));
            } else if (y.calisabilir) eylem.push(e('button', { sinif: 'i-d kucuk', metin: 'Onaydan geri çek', onclick: function() { durumYap(g, 'acik'); } }));
        }
        if (!acik && (y.onaylayabilir || y.yonetebilir)) eylem.push(e('button', { sinif: 'i-d kucuk', metin: 'Yeniden aç', onclick: function() { durumYap(g, 'acik'); } }));
        if (acik && y.yonetebilir) eylem.push(e('button', { sinif: 'i-d kucuk tehlike', metin: 'İptal et', onclick: function(ev) {
            const b = ev.currentTarget;
            if (!b.dataset.onay) { b.dataset.onay = '1'; b.textContent = 'Emin misiniz?'; setTimeout(function() { delete b.dataset.onay; b.textContent = 'İptal et'; }, 4000); return; }
            durumYap(g, 'iptal');
        } }));
        if (eylem.length) ic.push(e('div', { sinif: 'kutu' }, [g.kontrol && acik ? e('div', { stil: 'font-weight:600;margin-bottom:2px', metin: g.kontrol }) : null, e('div', { sinif: 'eylem', stil: 'margin-top:0' }, eylem)]));
        // Erteleme
        if (acik && y.calisabilir && !g.erteleme_bekliyor && g.durum !== 'onayda') {
            const t = e('input', { type: 'date', value: ertesiIsGunu(g.son_tarih < d.bugun ? d.bugun : g.son_tarih) });
            const gerekce = e('input', { type: 'text', placeholder: 'Gerekçe', stil: 'flex:1;min-width:120px' });
            const ertele = async function(tarih) {
                try {
                    const v = await istek('ertele', { id: g.id, yeni_tarih: tarih, gerekce: gerekce.value });
                    bildir(v.uygulandi ? 'İş ' + gunMetni(tarih) + ' tarihine aktarıldı.' : 'Erteleme talebi onaya gönderildi.');
                    yukle(); ozetTazele(); isAc(g.id);
                } catch (x) { bildir(x.message); }
            };
            ic.push(e('div', { sinif: 'kutu' }, [e('div', { sinif: 'bolum', stil: 'margin-top:0', metin: 'Ertele' }),
                e('div', { sinif: 'eylem', stil: 'margin-top:0' }, [
                    e('button', { sinif: 'i-d kucuk', metin: 'Ertesi güne aktar', onclick: function() { ertele(ertesiIsGunu(g.son_tarih < d.bugun ? d.bugun : g.son_tarih)); } }),
                    t, gerekce, e('button', { sinif: 'i-d kucuk', metin: 'Ertele', onclick: function() { ertele(t.value); } })]),
                g.onay_gerekir && !y.onaylayabilir ? e('div', { sinif: 'alt', metin: 'Onaylı işte erteleme onaylayana gider.' }) : null]));
        }
        (g.ertelemeler || []).filter(function(x) { return x.durum === 'bekliyor'; }).forEach(function(x) {
            ic.push(e('div', { sinif: 'kutu vurgu' }, [e('div', { metin: (x.isteyen || '') + ' erteleme istedi: ' + gunMetni(x.eski_tarih) + ' → ' + gunMetni(x.yeni_tarih) }),
                x.gerekce ? e('div', { sinif: 'alt', metin: x.gerekce }) : null,
                y.onaylayabilir ? e('div', { sinif: 'eylem' }, [true, false].map(function(kabul) {
                    return e('button', { sinif: 'i-d kucuk' + (kabul ? ' ana' : ''), metin: kabul ? 'Kabul et' : 'Reddet', onclick: async function() {
                        try { await istek('erteleme_karar', { id: x.id, kabul: kabul }); bildir('Kaydedildi.'); yukle(); ozetTazele(); isAc(g.id); } catch (z) { bildir(z.message); }
                    } });
                })) : e('div', { sinif: 'alt', metin: 'Karar bekleniyor.' })]));
        });
        // Alt işler
        if (!g.ust_gorev_id) {
            const altlar = g.altlar || [];
            const kutu = e('div', { sinif: 'kutu' }, [e('div', { sinif: 'bolum', stil: 'margin-top:0', metin: 'Alt işler' + (altlar.length ? ' (' + altlar.filter(function(a) { return a.durum === 'tamamlandi'; }).length + '/' + altlar.length + ')' : '') })]);
            altlar.forEach(function(a) {
                const kutucuk = e('input', { type: 'checkbox' }); kutucuk.checked = a.durum === 'tamamlandi';
                kutucuk.disabled = !(y.calisabilir || y.onaylayabilir) || a.durum === 'iptal';
                kutucuk.addEventListener('change', async function() {
                    try { await istek('durum', { id: a.id, durum: kutucuk.checked ? 'tamamla' : 'acik' }); isAc(g.id); yukle(); } catch (x) { bildir(x.message); kutucuk.checked = !kutucuk.checked; }
                });
                kutu.appendChild(e('label', { sinif: 'i-onay', stil: 'color:inherit;margin:3px 0' }, [kutucuk,
                    e('span', { stil: a.durum === 'tamamlandi' ? 'text-decoration:line-through;opacity:.7' : '', metin: a.baslik + (a.sorumlu_adi && a.sorumlu_id !== g.sorumlu_id ? ' — ' + a.sorumlu_adi : '') })]));
            });
            if (acik && (y.calisabilir || y.yonetebilir)) {
                const yeni = e('input', { type: 'text', placeholder: 'Alt iş ekle (Enter)', stil: 'width:100%;margin-top:6px' });
                yeni.addEventListener('keydown', async function(ev) {
                    if (ev.key !== 'Enter' || !yeni.value.trim()) return;
                    try { await istek('ac', { baslik: yeni.value.trim(), ust_gorev_id: g.id }); isAc(g.id); yukle(); } catch (x) { bildir(x.message); }
                });
                kutu.appendChild(yeni);
            }
            if (altlar.length || acik) ic.push(kutu);
        }
        // Hatırlatıcı: kişi kendine kurar; zamanı gelince sayfada uyarı çıkar.
        const hl = g.hatirlaticilar || [];
        if (hl.length || acik) {
            const hk = e('div', { sinif: 'kutu' }, [e('div', { sinif: 'bolum', stil: 'margin-top:0', metin: 'Hatırlatıcı' })]);
            hl.forEach(function(h) {
                hk.appendChild(e('div', { sinif: 'sat' }, [e('span', { metin: '🔔 ' + kisaZaman(h.zaman) }), e('b', {}, [h.notu || '',
                    e('button', { sinif: 'i-d kucuk', metin: '✕', title: 'Hatırlatıcıyı sil', 'aria-label': 'Hatırlatıcıyı sil', stil: 'margin-left:6px', onclick: async function() {
                        try { await istek('hatirlatici_sil', { id: h.id }); bildir('Hatırlatıcı silindi.'); isAc(g.id); yukle(); ozetTazele(); } catch (x) { bildir(x.message); }
                    } })])]));
            });
            if (acik) {
                const sec = hatirlatSecici(g.son_tarih);
                const not = e('input', { type: 'text', maxlength: '500', placeholder: 'Not (isteğe bağlı) — ör. “Müvekkili ara”', stil: 'flex:1;min-width:140px' });
                hk.appendChild(e('div', { stil: hl.length ? 'margin-top:8px' : '' }, [sec.el]));
                hk.appendChild(e('div', { sinif: 'eylem' }, [not, e('button', { sinif: 'i-d ana kucuk', metin: 'Bana hatırlat', onclick: async function() {
                    const z = sec.deger();
                    if (!z) { bildir('Ne zaman hatırlatılacağını seçin.'); return; }
                    if (gecmisZaman(z)) { bildir('Hatırlatma zamanı geçmişte olamaz.'); return; }
                    try {
                        await istek('hatirlatici', { gorev_id: g.id, zaman: z, notu: not.value });
                        bildir('Hatırlatıcı kuruldu: ' + kisaZaman(z));
                        isAc(g.id); yukle();
                    } catch (x) { bildir(x.message); }
                } })]));
                hk.appendChild(e('div', { sinif: 'alt', metin: 'Zamanı gelince Çoklu UYAP\'ta yalnız size uyarı çıkar.' }));
            }
            ic.push(hk);
        }
        // Notlar
        const notKutu = e('div', { sinif: 'kutu' }, [e('div', { sinif: 'bolum', stil: 'margin-top:0', metin: 'Notlar' })]);
        (g.notlar || []).forEach(function(n) { notKutu.appendChild(e('div', { sinif: 'not' }, [e('div', { sinif: 'alt', metin: (n.yazan || '') + ' · ' + gunMetni(n.zaman) + (n.zaman ? n.zaman.slice(10) : '') }), n.metin])); });
        if (y.calisabilir || y.onaylayabilir || y.yonetebilir) {
            const metin = e('textarea', { placeholder: 'Not yazın' });
            notKutu.appendChild(metin);
            notKutu.appendChild(e('div', { sinif: 'eylem' }, [e('button', { sinif: 'i-d kucuk', metin: 'Not ekle', onclick: async function() {
                if (!metin.value.trim()) return;
                try { await istek('not', { id: g.id, metin: metin.value }); isAc(g.id); } catch (x) { bildir(x.message); }
            } })]));
        }
        ic.push(notKutu);
        ic.push(belgeKutusu(g, y));
        ic.push(ekKutusu(g, y));
        // Kişiler (ek sorumlu / onaylayıcı / izleyici)
        if (!g.ust_gorev_id && (y.yonetebilir || (g.kisiler || []).length)) {
            const kk = e('div', { sinif: 'kutu' }, [e('div', { sinif: 'bolum', stil: 'margin-top:0', metin: 'Kişiler' })]);
            const rolAdi = { sorumlu: 'Ek sorumlu', onaylayici: 'Onaylayıcı', izleyici: 'İzleyici' };
            (g.kisiler || []).forEach(function(k) {
                kk.appendChild(e('div', { sinif: 'sat' }, [e('span', { metin: rolAdi[k.rol] || k.rol }), e('b', {}, [kisiAdi(k.id) || k.ad, y.yonetebilir ? e('button', { sinif: 'i-d kucuk', metin: '✕', stil: 'margin-left:6px', onclick: async function() {
                    try { await istek('kisi_cikar', { id: g.id, kullanici: k.id, rol: k.rol }); isAc(g.id); } catch (x) { bildir(x.message); }
                } }) : null])]));
            });
            if (y.yonetebilir) {
                const kisi = e('select', {}, d.kisiler.map(function(k) { return secenek(k.id, k.ad, ''); }));
                const rol = e('select', {}, [secenek('onaylayici', 'Onaylayıcı', ''), secenek('sorumlu', 'Ek sorumlu', ''), secenek('izleyici', 'İzleyici', '')]);
                kk.appendChild(e('div', { sinif: 'eylem' }, [kisi, rol, e('button', { sinif: 'i-d kucuk', metin: 'Ekle', onclick: async function() {
                    try { await istek('kisi_ekle', { id: g.id, kullanici: Number(kisi.value), rol: rol.value }); isAc(g.id); } catch (x) { bildir(x.message); }
                } })]));
            }
            ic.push(kk);
        }
        // Düzenle
        if (y.yonetebilir || y.calisabilir) {
            const dz = e('div', { sinif: 'kutu', stil: 'display:none' });
            const baslik = e('input', { type: 'text', value: g.baslik });
            const aciklama = e('textarea', {}, [g.aciklama || '']);
            const tarih = e('input', { type: 'date', value: g.son_tarih, disabled: !y.yonetebilir });
            const saat = e('input', { type: 'time', value: g.son_saat || '' });
            const sorumlu = e('select', { disabled: !y.yonetebilir }, d.kisiler.map(function(k) { return secenek(k.id, k.ad, g.sorumlu_id); }));
            const oncelik = e('select', {}, [secenek(0, 'Düşük', g.oncelik), secenek(1, 'Normal', g.oncelik), secenek(2, 'Yüksek', g.oncelik)]);
            const onay = e('input', { type: 'checkbox', disabled: !y.yonetebilir }); onay.checked = g.onay_gerekir;
            const etiket = etiketSecici(g.etiketler || []);
            dz.appendChild(e('div', { sinif: 'form' }, [e('label', { sinif: 'b', metin: 'Başlık' }), baslik, e('label', { sinif: 'b', metin: 'Etiket' }), etiket,
                e('label', { sinif: 'b', metin: 'Açıklama' }), aciklama,
                e('label', { sinif: 'b', metin: 'Son tarih' }), e('div', { stil: 'display:flex;gap:8px' }, [tarih, saat]),
                e('label', { sinif: 'b', metin: 'Sorumlu' }), sorumlu, e('label', { sinif: 'b', metin: 'Öncelik' }), oncelik,
                e('span'), e('label', { sinif: 'i-onay' }, [onay, 'Onay gerekir']),
                e('div', { sinif: 'tam eylem' }, [e('button', { sinif: 'i-d ana kucuk', metin: 'Kaydet', onclick: async function() {
                    const gv = { id: g.id, baslik: baslik.value, aciklama: aciklama.value, son_saat: saat.value, oncelik: Number(oncelik.value), etiketler: etiket.deger() };
                    if (y.yonetebilir) {
                        if (tarih.value !== g.son_tarih) gv.son_tarih = tarih.value;
                        if (Number(sorumlu.value) !== g.sorumlu_id) gv.sorumlu_id = Number(sorumlu.value);
                        if (onay.checked !== g.onay_gerekir) gv.onay_gerekir = onay.checked;
                    }
                    try { await istek('guncelle', gv); bildir('Kaydedildi.'); yukle(); isAc(g.id); } catch (x) { bildir(x.message); }
                } })])]));
            ic.push(e('button', { sinif: 'i-d kucuk', metin: 'Düzenle', stil: 'margin-bottom:10px', onclick: function(ev) { dz.style.display = dz.style.display ? '' : 'none'; ev.currentTarget.remove(); } }));
            ic.push(dz);
        }
        // Geçmiş
        if ((g.gecmis || []).length) {
            const gk = e('details', { sinif: 'kutu' }, [e('summary', { stil: 'cursor:pointer', metin: 'Geçmiş (' + g.gecmis.length + ')' })]);
            g.gecmis.forEach(function(o) { gk.appendChild(e('div', { sinif: 'olay' }, [e('div', { sinif: 'alt', metin: gunMetni(o.zaman) + (o.zaman ? o.zaman.slice(10) : '') + ' · ' + (o.kisi || 'sistem') }), olayMetni(o)])); });
            ic.push(gk);
        }
        const chipler = [e('span', { sinif: 'chip', stil: '--r:' + (DURUM_RENK[g.durum] || '#8a877e'), metin: g.durum_adi })];
        if (g.onay_gerekir) chipler.push(e('span', { sinif: 'chip', metin: 'Onaylı iş' }));
        if (g.gizli) chipler.push(e('span', { sinif: 'chip', metin: 'Gizli' }));
        if (g.tur !== 'is') chipler.push(e('span', { sinif: 'chip', metin: adlar(d.sabitler && d.sabitler.turler, g.tur) }));
        etiketCipleri(g.etiketler).forEach(function(c) { chipler.push(c); });
        const dugmeler = [];
        if (g.uyap_karti) dugmeler.push(e('button', { sinif: 'i-d kucuk', metin: 'UYAP\'ta aç', title: 'Dosyayı UYAP\'ta bulup kartını açar', onclick: function() { uyaptaAc(g.uyap_karti); } }));
        if (g.ust_gorev_id) dugmeler.push(e('button', { sinif: 'i-d kucuk', metin: 'Ana iş', onclick: function() { isAc(g.ust_gorev_id); } }));
        d.panel = { baslik: g.baslik, chipler: chipler, dugmeler: dugmeler, icerik: ic };
        panelCiz();
    }
    function prompt2(m) { try { return window.prompt(m) || ''; } catch (x) { return ''; } }
    function olayMetni(o) {
        const a = o.ayrinti || {};
        const tur = { acildi: 'İş açıldı', guncellendi: 'Düzenlendi', durum: 'Durum değişti', sorumlu: 'Sorumlu değişti', kisi_eklendi: 'Kişi eklendi',
            kisi_cikti: 'Kişi çıkarıldı', not: 'Not eklendi', ek: 'Belge eklendi', erteleme_istendi: 'Erteleme istendi', ertelendi: 'Ertelendi',
            erteleme_reddedildi: 'Erteleme reddedildi', sonraki_acildi: 'Sonraki iş açıldı', tasindi: 'Buluttan taşındı', hatirlatildi: 'Onay hatırlatıldı' }[o.tur] || o.tur;
        let ek = '';
        if (o.tur === 'durum' && a.yeni) ek = ': ' + adlar(d.sabitler && d.sabitler.durumlar, a.yeni) + (a.sonuc ? ' (' + adlar(d.sabitler && d.sabitler.sonuclar, a.sonuc) + ')' : '');
        else if ((o.tur === 'ertelendi' || o.tur === 'erteleme_istendi') && a.yeni_tarih) ek = ': ' + gunMetni(a.yeni_tarih);
        else if (o.tur === 'hatirlatildi' && a.kime) ek = ': ' + a.kime.join(', ') + (a.notu ? ' — ' + a.notu : '');
        else if (a.eski_tur) ek = ' (eski kayıt)';
        return tur + ek;
    }

    // UYAP'ta aç: kart bu sayfanın üstünde açılır (sayfa kapanmaz); kart kapanınca kullanıcı buraya döner.
    async function uyaptaAc(kart) {
        if (!window.__uyapTazeKart || !window.__uyapKartiAcDogrudan) { bildir('Dosya kartı bu sayfadan açılamıyor.'); return; }
        if (!document.getElementById('searchId')) {
            history.pushState({}, '', '/');
            window.dispatchEvent(new PopStateEvent('popstate', { state: {} }));
            const bas = Date.now();
            while (!document.getElementById('searchId') && Date.now() - bas < 8000) await new Promise(function(r) { setTimeout(r, 150); });
            if (!document.getElementById('searchId')) { bildir('UYAP ana sayfası açılamadı.'); return; }
        }
        bildir('Dosya UYAP\'ta aranıyor…');
        const t = await window.__uyapTazeKart(kart).catch(function() { return null; });
        if (!t || !t.dosyaId) { bildir('Dosya UYAP\'ta bulunamadı: ' + (kart.birimAdi || '') + ' ' + (kart.dosyaNo || '')); return; }
        if (window.__uyapDosyaAnahtari) buradanAcilan.add(window.__uyapDosyaAnahtari(t));
        if (!window.__uyapKartiAcDogrudan(t)) bildir('Dosya kartı açılamadı (UYAP sayfası değişmiş olabilir).');
    }

    // ── Bağlantılar ──
    document.addEventListener('click', function(ev) {
        if (gorunum !== 'acik') return;
        const a = ev.target.closest && ev.target.closest('#av-sidebar-wrapper a, #av-sidebar-wrapper button.sideLink');
        if (a && !a.closest('#cu-menu-isler') && !a.closest('.fav-menu-icon')) kucult();
    }, true);
    document.addEventListener('keydown', function(ev) {
        if (onde() && ev.key === 'Escape' && d.panel && !ev.target.closest('input,select,textarea')) { d.panel = null; d.secili = null; d.oneri = null; panelCiz(); }
    });
    function kartDinle() {
        if (!window.__cuKartKapaninca) return false;
        window.__cuKartKapaninca(function(anahtar) {
            if (!buradanAcilan.has(anahtar)) return;
            buradanAcilan.delete(anahtar);
            if (gorunum === 'kucuk') ac(false);
        });
        return true;
    }
    if (!kartDinle()) setTimeout(kartDinle, 1000);
    window.addEventListener('resize', yerlestir);
    setInterval(function() { menuGirisi(); yerlestir(); gorevGuncelle(false); }, 700);
    // Bildirim yoklaması: sayfa kapalıyken de (rozet + uyarı kartları). 15 sn'de bir aralık denetlenir.
    setInterval(function() { if (Date.now() - yoklamaSon >= yoklamaAraligi() - 1000) yokla(false); }, 15000);
    document.addEventListener('visibilitychange', function() {
        if (document.visibilityState === 'visible' && Date.now() - yoklamaSon > 20000) yokla(false);
    });
    setTimeout(function() { yokla(false); }, 2500);
    menuGirisi();
    try {
        const v = sessionStorage.getItem(ACIK_ANAHTAR);
        if (v === '1') setTimeout(function() { ac(false); }, 300);
        else if (v === 'k') { gorunum = 'kucuk'; setTimeout(function() { gorevGuncelle(true); }, 300); }
    } catch (x) {}
    // Başka ekranlardan açma: window.__cuIslerAc() — ya da dosya bağıyla yeni iş: window.__cuIsAta({dosya, baslik, …})
    window.__cuIslerAc = function() { ac(true); };
    function hazirOlunca(fn) {
        const bekle = function(n) { if (d.ben && govde) fn(); else if (n > 0) setTimeout(function() { bekle(n - 1); }, 200); };
        bekle(50);
    }
    window.__cuIsAta = function(on) {
        on = on || {};
        ac(true);
        hazirOlunca(function() { d.donus = on.donus_kart || null; yeniIs(on); });
    };
    window.__cuIsGoster = function(id) {
        ac(true);
        hazirOlunca(function() { isAc(id); });
    };
    window.__cuIsDosya = function(ds) {
        ac(true);
        hazirOlunca(function() {
            d.dosyaSuzgec = { dosya_no: ds.dosya_no || '', birim_adi: ds.birim_adi || '' };
            d.donus = ds.donus_kart || null;
            d.sekme = 'tum'; d.filtre.durum = 'acik'; d.filtre.ara = ''; d.panel = null;
            ust(); yukle();
        });
    };
})();
