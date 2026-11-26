// ═══════════════════════════════════════════════════════════════════════════════════════
// ÇOKLU UYAP — AYARLAR SAYFASI (2026-11-26)
// ═══════════════════════════════════════════════════════════════════════════════════════
// İş Listesi / İcra Takip Süreçleri / Büro Belgeleri gibi pencere davranır (kullanıcı isteği 26.11): üst çubukta
// dişli düğmesi ve sol menüde (ÇOKLU UYAP) "Ayarlar"; görev çubuğunda kendi düğmesi (kaynak 'ayarlar'), küçült /
// öne getir; öbür sayfalardan biri açılınca bu küçülür. Solda bölümler, sağda içerik.
// Bölümler:
//   • IBAN'larım (vekil): birden çok, etiketli, bir varsayılan. Evrak listesinde tahsilat makbuzuna sağ tık →
//     "Parayı iste → Kayıtlı IBAN'ıma" bu listeyi kullanır (uyap_proxy.rs).
//   • Alacaklı IBAN'ları: dosya başına kayıtlar (sağ tık penceresinde girilir); burada görülür, silinir.
//   • Antetlerim: büro antetleri (src/antet.rs); Yazı editörünün "Antet" menüsü kullanır, varsayılan yeni belgeye konur.
// Veri ofis bilgisayarındaki büro veritabanında (/__cu__/veri/ibanlar…; src/iban.rs). UYAP'a istek atılmaz.
// UYAP'ın kendi "IBAN Bilgilerim" listesi IBAN'ı yıldızlı döndürdüğü için oradan içe aktarma yok.
(function() {
    'use strict';
    const anaCerceve = window.__uyapAnaCerceve ? window.__uyapAnaCerceve() : window.top === window;
    if (!anaCerceve || window.__cuAyarlar) return;
    window.__cuAyarlar = true;

    const UC = '/__cu__/veri/';
    const ACIK_ANAHTAR = 'cuAyarlarAcik';
    const BOLUM_ANAHTAR = 'cuAyarlarBolum';
    const DISLI = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">' +
        '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/></svg>';

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
    const dar = function() { return !!(window.__cuDar && window.__cuDar()); };
    function oku(k, v) { try { const x = sessionStorage.getItem(k); return x === null ? v : x; } catch (y) { return v; } }
    function yaz(k, v) { try { if (v === null) sessionStorage.removeItem(k); else sessionStorage.setItem(k, v); } catch (y) {} }
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
        if (!j.ok) throw new Error(j.kod === 'E-402' ? 'Büro veritabanı hazır değil (E-402). Ofis bilgisayarında program açık olmalı.'
            : (j.error || 'İşlem yapılamadı.'));
        return j.veri;
    }
    // iban.rs::duzelt ile aynı kural: "TR" + 24 rakam, mod-97 (sunucu da doğrular).
    function ibanDuzelt(ham) {
        let s = String(ham || '').replace(/[\s-]/g, '').toUpperCase();
        if (/^\d{24}$/.test(s)) s = 'TR' + s;
        if (!/^TR\d{24}$/.test(s)) return null;
        const d = s.slice(4) + '2927' + s.slice(2, 4);
        let k = 0;
        for (let i = 0; i < d.length; i++) k = (k * 10 + (d.charCodeAt(i) - 48)) % 97;
        return k === 1 ? s : null;
    }
    function kucukHarf(s) { return String(s || '').toLocaleLowerCase('tr-TR'); }

    // ── Stil (İş Listesi / Büro Belgeleri ile aynı tema değişkenleri) ──
    function stilKur() {
        if (document.getElementById('cu-ayarlar-stil')) return;
        const css = [
            '#cu-ayarlar{--a-zemin:var(--cu-t-zemin,#f3f4f7);--a-yuzey:var(--cu-t-yuzey,#fff);--a-serit:var(--cu-t-serit,#f5f6fa);',
            '--a-kenar:var(--cu-t-kenar,#dee2e6);--a-yazi:var(--cu-t-yazi,#212529);--a-vurgu:var(--cu-vurgu,#2d4596);',
            '--a-vurgu-koyu:var(--cu-vurgu-koyu,#1f3277);--a-vurgu-zemin:var(--cu-vurgu-zemin,#e8ecf8);',
            '--a-soluk:color-mix(in srgb,var(--a-yazi) 74%,var(--a-yuzey));--a-silik:color-mix(in srgb,var(--a-yazi) 64%,var(--a-yuzey));',
            'position:fixed;z-index:1400;top:var(--cu-nav-yuk,60px);right:0;bottom:var(--cu-cubuk-yuk,44px);left:0;',
            'background:var(--a-zemin);color:var(--a-yazi);font:14px/1.45 "Segoe UI",system-ui,sans-serif;display:flex;flex-direction:column;overflow:hidden}',
            'html[data-cu-tema="gunes"] #cu-ayarlar{--a-vurgu-koyu:var(--cu-vurgu-yazi,#6b4a00)}',
            'html[data-cu-tema="koyu"] #cu-ayarlar{--a-zemin:#1d2130;--a-yuzey:#2a2f40;--a-serit:#262b3b;--a-kenar:#3a4055;--a-yazi:#dedede;',
            '--a-vurgu:#3a55b5;--a-vurgu-koyu:#8ea2e6;--a-vurgu-zemin:#2c3554}',
            '#cu-ayarlar *{box-sizing:border-box}',
            '#cu-ayarlar button,#cu-ayarlar input{font:inherit;color:inherit}',
            // UYAP'ın "label{color:…!important}" kuralı bizim renklerimizi eziyordu (canlı 26.11)
            '#cu-ayarlar label{color:var(--a-soluk)!important;font-size:12.5px;font-weight:600}',
            '#cu-ayarlar .a-bas{display:flex;align-items:center;gap:14px;padding:14px 22px 12px;flex-wrap:wrap;border-bottom:1px solid var(--a-kenar)}',
            '#cu-ayarlar .a-bas h1{font-size:19px;margin:0;font-weight:600}',
            '#cu-ayarlar .a-bosluk{flex:1}',
            '#cu-ayarlar .a-d{border:1px solid var(--a-kenar);background:var(--a-yuzey);border-radius:9px;padding:7px 13px;cursor:pointer;white-space:nowrap}',
            '#cu-ayarlar .a-d:hover{border-color:var(--a-vurgu)}',
            '#cu-ayarlar .a-d.ana{background:var(--a-vurgu);border-color:var(--a-vurgu);color:var(--cu-vurgu-uzeri,#fff)}',
            '#cu-ayarlar .a-d.kucuk{padding:4px 9px;font-size:12.5px;border-radius:8px}',
            '#cu-ayarlar .a-d.tehlike{color:#b84444}',
            '#cu-ayarlar .a-d.tehlike.emin{background:#b84444;border-color:#b84444;color:#fff}',
            '#cu-ayarlar .a-govde{flex:1;display:flex;min-height:0}',
            '#cu-ayarlar .a-nav{width:240px;flex:none;overflow:auto;border-right:1px solid var(--a-kenar);background:var(--a-yuzey);padding:10px 8px}',
            '#cu-ayarlar .a-nav button{display:flex;align-items:center;gap:10px;width:100%;border:0;background:none;text-align:left;padding:9px 12px;border-radius:8px;cursor:pointer}',
            '#cu-ayarlar .a-nav button:hover{background:var(--a-serit)}',
            '#cu-ayarlar .a-nav button.sec{background:var(--a-vurgu-zemin);color:var(--a-vurgu-koyu);font-weight:600}',
            '#cu-ayarlar .a-nav .a-sayi{margin-left:auto;color:var(--a-silik);font-size:12px;font-weight:400}',
            '#cu-ayarlar .a-ana{flex:1;overflow:auto;padding:18px 26px 26px;min-width:0}',
            '#cu-ayarlar .a-ana h2{font-size:17px;margin:0 0 4px;font-weight:600}',
            '#cu-ayarlar .a-not{color:var(--a-soluk);font-size:13px;margin:0 0 14px;max-width:760px}',
            '#cu-ayarlar .a-kart{background:var(--a-yuzey);border:1px solid var(--a-kenar);border-radius:12px;padding:14px 16px;margin-bottom:16px;max-width:900px}',
            '#cu-ayarlar .a-kart h3{font-size:14px;margin:0 0 10px;font-weight:600}',
            '#cu-ayarlar .a-form{display:grid;grid-template-columns:minmax(260px,1.3fr) minmax(180px,1fr) auto;gap:10px;align-items:end}',
            '#cu-ayarlar .a-form label{display:flex;flex-direction:column;gap:4px}',
            '#cu-ayarlar input[type=text],#cu-ayarlar input[type=search]{background:var(--a-yuzey);border:1px solid var(--a-kenar);border-radius:9px;padding:8px 10px;min-width:0;width:100%}',
            '#cu-ayarlar input:focus{outline:2px solid color-mix(in srgb,var(--a-vurgu) 45%,transparent);outline-offset:1px;border-color:var(--a-vurgu)}',
            '#cu-ayarlar .mono{font-family:Consolas,"Courier New",monospace;letter-spacing:.3px}',
            '#cu-ayarlar table{width:100%;border-collapse:separate;border-spacing:0;background:var(--a-yuzey);border:1px solid var(--a-kenar);border-radius:12px;overflow:hidden;max-width:900px}',
            '#cu-ayarlar th{text-align:left;font-weight:600;font-size:12px;color:var(--a-soluk);background:var(--a-serit);padding:8px 12px;white-space:nowrap}',
            '#cu-ayarlar td{padding:9px 12px;border-top:1px solid var(--a-kenar);vertical-align:middle}',
            '#cu-ayarlar tr.vars td{background:color-mix(in srgb,var(--a-vurgu) 7%,var(--a-yuzey))}',
            '#cu-ayarlar .a-rozet{display:inline-block;padding:1px 9px;border-radius:999px;font-size:12px;font-weight:600;background:var(--a-vurgu);color:var(--cu-vurgu-uzeri,#fff);white-space:nowrap}',
            '#cu-ayarlar .a-etiket{cursor:text;border-bottom:1px dashed transparent}',
            '#cu-ayarlar .a-etiket:hover{border-bottom-color:var(--a-silik)}',
            '#cu-ayarlar .a-etiket.bos{color:var(--a-silik);font-style:italic}',
            '#cu-ayarlar .a-eylem{display:flex;gap:6px;justify-content:flex-end;flex-wrap:wrap}',
            '#cu-ayarlar .a-bilgi{background:var(--a-yuzey);border:1px dashed var(--a-kenar);border-radius:12px;padding:22px;color:var(--a-soluk);text-align:center;max-width:900px}',
            '#cu-ayarlar .a-hata{background:color-mix(in srgb,#b84444 12%,var(--a-yuzey));color:color-mix(in srgb,#b84444 80%,var(--a-yazi));border-radius:10px;padding:9px 13px;margin:10px 0 0;max-width:900px}',
            '#cu-ayarlar .a-hata:empty{display:none}',
            '#cu-ayarlar .alt{color:var(--a-soluk);font-size:12.5px}',
            '#cu-ayarlar .a-arac{display:flex;gap:10px;align-items:center;margin-bottom:12px;max-width:900px}',
            '#cu-ayarlar .a-arac input[type=search]{max-width:420px}',
            '#cu-menu-ayarlar .SidebarLabel{font-weight:600}',
            '#cu-menu-ayarlar.on > li > a{background:var(--cu-vurgu-zemin,#e8ecf8)}',
            // Üst çubuk düğmesi: tema / mavi ışık düğmeleriyle aynı boy; UYAP'ın simge iç boşluğu sıfırlanır
            '#cu-ayar-dugme{display:inline-flex;align-items:center;justify-content:center;cursor:pointer}',
            '#cu-ayar-dugme svg{width:22px;height:22px;display:block;padding:0!important}',
            '#cu-ayar-dugme.on{color:var(--cu-vurgu-acik)}',
            // Dar ekran: bölümler üstte yatay, tablo kartlara döner
            'html.cu-dar #cu-ayarlar .a-bas{padding:10px 12px}',
            'html.cu-dar #cu-ayarlar .a-govde{flex-direction:column}',
            'html.cu-dar #cu-ayarlar .a-nav{width:auto;display:flex;gap:4px;overflow-x:auto;border-right:0;border-bottom:1px solid var(--a-kenar);padding:6px}',
            'html.cu-dar #cu-ayarlar .a-nav button{width:auto;white-space:nowrap}',
            'html.cu-dar #cu-ayarlar .a-ana{padding:12px}',
            'html.cu-dar #cu-ayarlar .a-form{grid-template-columns:1fr}',
            'html.cu-dar #cu-ayarlar table,html.cu-dar #cu-ayarlar tbody,html.cu-dar #cu-ayarlar tr,html.cu-dar #cu-ayarlar td{display:block;width:100%}',
            'html.cu-dar #cu-ayarlar thead{display:none}',
            'html.cu-dar #cu-ayarlar td{border-top:0;padding:3px 12px}',
            'html.cu-dar #cu-ayarlar tr{border-top:1px solid var(--a-kenar);padding:6px 0}',
            'html.cu-dar #cu-ayarlar tr:first-child{border-top:0}',
            'html.cu-dar #cu-ayarlar .a-eylem{justify-content:flex-start}',
        ].join('\n');
        document.head.appendChild(e('style', { id: 'cu-ayarlar-stil', metin: css }));
    }

    // ── Durum ──
    const BOLUMLER = [
        { kimlik: 'iban', ad: 'IBAN\'larım', ikon: '🏦', ciz: ibanCiz },
        { kimlik: 'alacakli', ad: 'Alacaklı IBAN\'ları', ikon: '📄', ciz: alacakliCiz },
        { kimlik: 'antet', ad: 'Antetlerim', ikon: '🖼️', ciz: antetCiz },
    ];
    let kok = null, navEl = null, anaEl = null, gorunum = 'kapali';
    let bolum = oku(BOLUM_ANAHTAR, 'iban');
    if (!BOLUMLER.some(function(b) { return b.kimlik === bolum; })) bolum = 'iban';
    const sayilar = {};

    function durumSakla() { yaz(ACIK_ANAHTAR, gorunum === 'kapali' ? null : (gorunum === 'acik' ? '1' : 'k')); }

    // ── Menü girişi ve üst çubuk düğmesi ──
    function menuGirisi() {
        const yan = document.getElementById('av-sidebar-wrapper');
        if (!yan || document.getElementById('cu-menu-ayarlar')) return;
        stilKur();
        const ust = yan.querySelector('.sidebar-top-part') || yan.firstElementChild || yan;
        const ul = e('ul', { id: 'cu-menu-ayarlar', sinif: 'menu-list' + (gorunum === 'acik' ? ' on' : '') }, [
            e('li', { sinif: 'SidebarLink' }, [
                e('a', { href: '#', title: 'Çoklu UYAP ayarları: IBAN\'larım, alacaklı IBAN\'ları, antetler',
                         onclick: function(ev) { ev.preventDefault(); ac(); } },
                  [e('span', { sinif: 'SidebarLabel', metin: 'Ayarlar' })])
            ])
        ]);
        // Bizim girişler (cu-menu-*) UYAP'ın menüsünden önce gelir; Ayarlar onların sonuna.
        const bizim = ust.querySelectorAll(':scope > ul[id^="cu-menu-"]');
        const son = bizim.length ? bizim[bizim.length - 1] : null;
        if (son) ust.insertBefore(ul, son.nextSibling); else ust.insertBefore(ul, ust.firstChild);
    }
    function dugmeKur() {
        let d = document.getElementById('cu-ayar-dugme');
        if (d) { d.classList.toggle('on', gorunum === 'acik'); return; }
        const t = document.getElementById('cu-tema-dugme');
        if (!t || !t.parentElement) return;
        stilKur();
        d = document.createElement(t.tagName === 'BUTTON' ? 'button' : 'a');
        d.id = 'cu-ayar-dugme';
        d.className = t.className;
        d.title = 'Ayarlar';
        d.setAttribute('role', 'button');
        d.setAttribute('aria-label', 'Ayarlar');
        if (d.tagName === 'BUTTON') d.type = 'button'; else d.href = 'javascript:void(0)';
        d.innerHTML = DISLI;
        d.addEventListener('click', function(ev) { ev.preventDefault(); ev.stopPropagation(); if (gorunum === 'acik') kucult(); else ac(); });
        const kap = t.parentElement;
        if (kap.tagName === 'LI') {
            const li = document.createElement('li');
            li.className = kap.className.replace(/\bcu-uyap-tema\b/, '').trim();
            li.appendChild(d);
            kap.parentElement.insertBefore(li, kap.nextSibling);
        } else {
            kap.insertBefore(d, t.nextSibling);
        }
    }

    // ── Pencere ──
    function yerlestir() {
        if (!kok) return;
        const ic = document.getElementById('page-content-wrapper');
        let sol = 0;
        if (ic && !dar()) sol = Math.max(0, Math.round(ic.getBoundingClientRect().left));
        if (kok.style.left !== sol + 'px') kok.style.left = sol + 'px';
    }
    const sayfalar = window.__cuSayfalar = window.__cuSayfalar || {};
    sayfalar['ayarlar'] = function() { if (gorunum === 'acik') kucult(); };
    function digerSayfalariIndir() {
        Object.keys(sayfalar).forEach(function(k) { if (k !== 'ayarlar') try { sayfalar[k](); } catch (x) {} });
    }
    function menuIsaretle() {
        const m = document.getElementById('cu-menu-ayarlar');
        if (m) m.classList.toggle('on', gorunum === 'acik');
        const d = document.getElementById('cu-ayar-dugme');
        if (d) d.classList.toggle('on', gorunum === 'acik');
    }
    function ac() {
        stilKur();
        gorunum = 'acik';
        durumSakla();
        if (!kok) {
            kok = e('div', { id: 'cu-ayarlar', role: 'region', 'aria-label': 'Ayarlar' });
            document.body.appendChild(kok);
            ciz();
        }
        digerSayfalariIndir();
        kok.style.display = '';
        menuIsaretle();
        yerlestir();
        gorevGuncelle(true);
    }
    function kucult() {
        if (gorunum === 'kapali') return;
        gorunum = 'kucuk';
        durumSakla();
        if (kok) kok.style.display = 'none';
        menuIsaretle();
        gorevGuncelle(true);
    }
    function kapat() {
        gorunum = 'kapali';
        durumSakla();
        if (kok) kok.remove();
        kok = null; navEl = null; anaEl = null;
        menuIsaretle();
        gorevGuncelle(true);
    }
    let gorevImza = null;
    function gorevTikla() { if (gorunum === 'acik') kucult(); else ac(); }
    function gorevGuncelle(zorla) {
        if (!window.CU || !window.CU.gorev) return;
        const imza = gorunum;
        if (!zorla && imza === gorevImza) return;
        gorevImza = imza;
        if (gorunum === 'kapali') { window.CU.gorev.ayarla([], 'ayarlar'); return; }
        window.CU.gorev.ayarla([{
            kimlik: 'ayarlar', ad: 'Ayarlar', ikon: '⚙️', on: gorunum === 'acik', kucuk: gorunum !== 'acik',
            tikla: gorevTikla, kapat: kapat,
            menu: function() { return [[[gorunum === 'acik' ? 'Küçült' : 'Öne getir', gorevTikla]]]; },
        }], 'ayarlar');
    }

    // ── Çizim ──
    function ciz() {
        if (!kok) return;
        bos(kok);
        kok.appendChild(e('div', { sinif: 'a-bas' }, [
            e('h1', { metin: 'Ayarlar' }), e('div', { sinif: 'a-bosluk' }),
            e('button', { sinif: 'a-d', metin: '–', title: 'Görev çubuğuna küçült', 'aria-label': 'Küçült', stil: 'min-width:38px;font-weight:700', onclick: kucult }),
            e('button', { sinif: 'a-d', metin: 'Kapat', title: 'Sayfayı kapat (görev çubuğundan da çıkar)', onclick: kapat })
        ]));
        navEl = e('nav', { sinif: 'a-nav', 'aria-label': 'Ayar bölümleri' });
        anaEl = e('div', { sinif: 'a-ana' });
        kok.appendChild(e('div', { sinif: 'a-govde' }, [navEl, anaEl]));
        navCiz();
        bolumCiz();
    }
    function navCiz() {
        if (!navEl) return;
        bos(navEl);
        BOLUMLER.forEach(function(b) {
            navEl.appendChild(e('button', { type: 'button', sinif: b.kimlik === bolum ? 'sec' : '', 'aria-current': b.kimlik === bolum ? 'page' : null,
                onclick: function() { bolum = b.kimlik; yaz(BOLUM_ANAHTAR, bolum); navCiz(); bolumCiz(); } },
                [e('span', { metin: b.ikon, 'aria-hidden': 'true' }), e('span', { metin: b.ad }),
                 sayilar[b.kimlik] != null ? e('span', { sinif: 'a-sayi', metin: String(sayilar[b.kimlik]) }) : null]));
        });
    }
    function bolumCiz() {
        if (!anaEl) return;
        bos(anaEl);
        const b = BOLUMLER.find(function(x) { return x.kimlik === bolum; }) || BOLUMLER[0];
        b.ciz(anaEl);
    }
    function sayiYaz(k, n) { if (sayilar[k] !== n) { sayilar[k] = n; navCiz(); } }
    // Silme iki adımlı (tarayıcı onay penceresi açılmaz): ilk tık "Emin misiniz?", ikinci tık siler.
    function silDugmesi(fn) {
        const d = e('button', { sinif: 'a-d kucuk tehlike', type: 'button', metin: 'Sil' });
        let zaman = null;
        d.addEventListener('click', function() {
            if (!zaman) {
                d.textContent = 'Emin misiniz?';
                d.classList.add('emin');
                zaman = setTimeout(function() { d.textContent = 'Sil'; d.classList.remove('emin'); zaman = null; }, 3000);
                return;
            }
            clearTimeout(zaman);
            fn();
        });
        return d;
    }

    // ── Bölüm: IBAN'larım (vekil) ──
    function ibanCiz(kap) {
        kap.appendChild(e('h2', { metin: 'IBAN\'larım' }));
        kap.appendChild(e('p', { sinif: 'a-not', metin: 'Vekil hesapları. Evrak listesinde tahsilat makbuzuna sağ tık → "Parayı iste → Kayıtlı IBAN\'ıma" ' +
            'bu listeden seçtirir; varsayılan IBAN listede ilk gelir. Kayıtlar ofis bilgisayarındaki büro veritabanında tutulur.' }));
        const hata = e('div', { sinif: 'a-hata', role: 'alert' });
        const ibanG = e('input', { type: 'text', sinif: 'mono', placeholder: 'TR00 0000 0000 0000 0000 0000 00', autocomplete: 'off', spellcheck: 'false' });
        const etiketG = e('input', { type: 'text', placeholder: 'ör. Ziraat — büro hesabı', maxlength: '120', autocomplete: 'off' });
        const ekle = async function() {
            hata.textContent = '';
            const iban = ibanDuzelt(ibanG.value);
            if (!iban) { hata.textContent = 'IBAN geçersiz: "TR" + 24 rakam olmalı ve kontrol hanesi tutmalı.'; ibanG.focus(); return; }
            try {
                await istek('iban_ekle', { iban: iban, etiket: etiketG.value });
                ibanG.value = etiketG.value = '';
                bildir('IBAN kaydedildi.');
                yukle();
            } catch (x) { hata.textContent = x.message; }
        };
        [ibanG, etiketG].forEach(function(i) { i.addEventListener('keydown', function(ev) { if (ev.key === 'Enter') { ev.preventDefault(); ekle(); } }); });
        kap.appendChild(e('div', { sinif: 'a-kart' }, [
            e('h3', { metin: 'Yeni IBAN' }),
            e('div', { sinif: 'a-form' }, [
                e('label', null, ['IBAN', ibanG]),
                e('label', null, ['Etiket (isteğe bağlı)', etiketG]),
                e('button', { sinif: 'a-d ana', type: 'button', onclick: ekle, metin: 'Ekle' })
            ]),
            hata
        ]));
        const liste = e('div', null, [e('div', { sinif: 'a-bilgi', metin: 'Yükleniyor…' })]);
        kap.appendChild(liste);

        async function yukle() {
            let l;
            try { l = await istek('ibanlar'); } catch (x) { bos(liste).appendChild(e('div', { sinif: 'a-hata', metin: x.message })); return; }
            l = Array.isArray(l) ? l : [];
            sayiYaz('iban', l.length);
            bos(liste);
            if (!l.length) { liste.appendChild(e('div', { sinif: 'a-bilgi', metin: 'Henüz IBAN kaydedilmedi. Yukarıdan ekleyin.' })); return; }
            const islem = async function(uc, govde, mesaj) {
                hata.textContent = '';
                try { await istek(uc, govde); if (mesaj) bildir(mesaj); yukle(); } catch (y) { hata.textContent = y.message; }
            };
            const tb = e('tbody');
            l.forEach(function(x) {
                const etiket = e('span', { sinif: 'a-etiket' + (x.etiket ? '' : ' bos'), metin: x.etiket || 'Etiket ekle', title: 'Düzenlemek için tıklayın', tabindex: '0' });
                // Etiket satır içinde düzenlenir: Enter / odak çıkınca kaydedilir, Esc vazgeçer.
                const duzenle = function() {
                    const i = e('input', { type: 'text', value: x.etiket || '', maxlength: '120' });
                    let bitti = false;
                    const kaydet = function() { if (bitti) return; bitti = true; islem('iban_etiket', { id: x.id, etiket: i.value }, 'Etiket kaydedildi.'); };
                    i.addEventListener('keydown', function(ev) {
                        if (ev.key === 'Enter') { ev.preventDefault(); kaydet(); }
                        else if (ev.key === 'Escape') { ev.stopPropagation(); bitti = true; yukle(); }
                    });
                    i.addEventListener('blur', kaydet);
                    etiket.replaceWith(i);
                    i.focus();
                };
                etiket.addEventListener('click', duzenle);
                etiket.addEventListener('keydown', function(ev) { if (ev.key === 'Enter') duzenle(); });
                tb.appendChild(e('tr', { sinif: x.varsayilan ? 'vars' : '' }, [
                    e('td', { sinif: 'mono', metin: x.gruplu }),
                    e('td', null, [etiket]),
                    e('td', null, [x.varsayilan ? e('span', { sinif: 'a-rozet', metin: 'Varsayılan' })
                        : e('button', { sinif: 'a-d kucuk', type: 'button', metin: 'Varsayılan yap', onclick: function() { islem('iban_varsayilan', { id: x.id }, 'Varsayılan IBAN değişti.'); } })]),
                    e('td', null, [e('div', { sinif: 'a-eylem' }, [silDugmesi(function() { islem('iban_sil', { id: x.id }, 'IBAN silindi.'); })])])
                ]));
            });
            liste.appendChild(e('table', null, [
                e('thead', null, [e('tr', null, [e('th', { metin: 'IBAN' }), e('th', { metin: 'Etiket' }), e('th', { metin: 'Durum' }), e('th', { metin: '' })])]),
                tb
            ]));
        }
        yukle();
        setTimeout(function() { ibanG.focus(); }, 0);
    }

    // ── Bölüm: Alacaklı IBAN'ları (dosya başına) ──
    function alacakliCiz(kap) {
        kap.appendChild(e('h2', { metin: 'Alacaklı IBAN\'ları' }));
        kap.appendChild(e('p', { sinif: 'a-not', metin: 'Dosya başına kayıtlı alacaklı hesapları. Tahsilat makbuzuna sağ tık → "Parayı iste → Alacaklı IBAN\'ına" ' +
            'penceresinde girilip kaydedilir; o dosyada bir sonraki talepte "Dosyada kayıtlı IBAN" olarak gelir.' }));
        const ara = e('input', { type: 'search', placeholder: 'Birim, dosya no ya da IBAN ara…', 'aria-label': 'Ara' });
        kap.appendChild(e('div', { sinif: 'a-arac' }, [ara]));
        const hata = e('div', { sinif: 'a-hata', role: 'alert' });
        const liste = e('div', null, [e('div', { sinif: 'a-bilgi', metin: 'Yükleniyor…' })]);
        kap.appendChild(liste);
        kap.appendChild(hata);
        let tum = [];
        function goster() {
            bos(liste);
            const q = kucukHarf(ara.value).replace(/\s+/g, '');
            const l = q ? tum.filter(function(x) { return kucukHarf([x.birim_adi, x.dosya_no, x.iban].join('')).replace(/\s+/g, '').indexOf(q) >= 0; }) : tum;
            if (!tum.length) { liste.appendChild(e('div', { sinif: 'a-bilgi', metin: 'Henüz hiçbir dosyaya alacaklı IBAN\'ı kaydedilmedi.' })); return; }
            if (!l.length) { liste.appendChild(e('div', { sinif: 'a-bilgi', metin: 'Aramayla eşleşen kayıt yok.' })); return; }
            const tb = e('tbody');
            l.forEach(function(x) {
                tb.appendChild(e('tr', null, [
                    e('td', null, [e('div', { metin: x.birim_adi || ('Birim ' + x.birim_id) }), e('div', { sinif: 'alt', metin: x.dosya_no })]),
                    e('td', { sinif: 'mono', metin: x.gruplu }),
                    e('td', { sinif: 'alt', metin: x.guncelleme || '' }),
                    e('td', null, [e('div', { sinif: 'a-eylem' }, [silDugmesi(async function() {
                        hata.textContent = '';
                        try { await istek('alacakli_iban_sil', { birim_id: x.birim_id, dosya_no: x.dosya_no }); bildir('Alacaklı IBAN\'ı silindi.'); yukle(); }
                        catch (y) { hata.textContent = y.message; }
                    })])])
                ]));
            });
            liste.appendChild(e('table', null, [
                e('thead', null, [e('tr', null, [e('th', { metin: 'Dosya' }), e('th', { metin: 'IBAN' }), e('th', { metin: 'Güncelleme' }), e('th', { metin: '' })])]),
                tb
            ]));
        }
        async function yukle() {
            try { tum = await istek('alacakli_ibanlar'); } catch (x) { bos(liste).appendChild(e('div', { sinif: 'a-hata', metin: x.message })); return; }
            tum = Array.isArray(tum) ? tum : [];
            sayiYaz('alacakli', tum.length);
            goster();
        }
        ara.addEventListener('input', goster);
        yukle();
    }

    // ── Bölüm: Antetlerim (büro geneli) ──
    // Resim UYAP antedi biçimine çevrilir (armonia-yazi antet.ts resimdenAntet ile aynı kural): PNG, 1 piksel = 1 pt,
    // en çok 453 pt genişlik (UYAP'ın yeni belgesinde yazı alanı: A4 − 2×2,5 cm) ve 170 pt yükseklik, oran korunur.
    const ANTET_GEN = 453, ANTET_YUK = 170;
    function antetHazirla(dosya) {
        return new Promise(function(tamam, red) {
            const url = URL.createObjectURL(dosya);
            const img = new Image();
            img.onload = function() {
                try {
                    const k = Math.min(1, ANTET_GEN / img.naturalWidth, ANTET_YUK / img.naturalHeight);
                    const w = Math.max(1, Math.round(img.naturalWidth * k)), h = Math.max(1, Math.round(img.naturalHeight * k));
                    const t = document.createElement('canvas');
                    t.width = w; t.height = h;
                    const c = t.getContext('2d');
                    c.imageSmoothingQuality = 'high';
                    c.drawImage(img, 0, 0, w, h);
                    tamam({ resim: t.toDataURL('image/png').replace(/^data:image\/png;base64,/, ''), genislik: w, yukseklik: h });
                } catch (x) { red(new Error('Resim işlenemedi.')); }
                URL.revokeObjectURL(url);
            };
            img.onerror = function() { URL.revokeObjectURL(url); red(new Error('Resim açılamadı (PNG, JPEG, GIF, BMP ya da WebP olmalı).')); };
            img.src = url;
        });
    }
    function antetResmi(a, enCok) {
        const i = e('img', { alt: 'Antet: ' + (a.ad || ''), src: 'data:image/png;base64,' + a.resim,
            stil: 'display:block;max-width:' + enCok + 'px;width:' + a.genislik + 'px;height:auto;background:#fff;border:1px solid var(--a-kenar);border-radius:6px;padding:4px' });
        return i;
    }
    function antetCiz(kap) {
        kap.appendChild(e('h2', { metin: 'Antetlerim' }));
        kap.appendChild(e('p', { sinif: 'a-not', metin: 'Belgelerin üstündeki büro resmi. Yazı editöründe "Antet" menüsünden eklenir; UYAP\'taki gibi sayfa ' +
            'üst bilgisine konur. Varsayılan antet seçerseniz yeni belgeler onunla açılır. Bir belgedeki anteti editörde ' +
            '"Antet → Bu anteti büroya kaydet" ile de buraya alabilirsiniz. Büroda herkes kullanır.' }));
        const hata = e('div', { sinif: 'a-hata', role: 'alert' });
        const adG = e('input', { type: 'text', placeholder: 'ör. Büro anteti', maxlength: '120', autocomplete: 'off' });
        const dosyaG = e('input', { type: 'file', accept: 'image/png,image/jpeg,image/gif,image/bmp,image/webp', stil: 'max-width:100%' });
        const varsG = e('input', { type: 'checkbox' });
        const onizleme = e('div', { stil: 'margin-top:10px' });
        let hazir = null;
        dosyaG.addEventListener('change', async function() {
            hata.textContent = '';
            hazir = null;
            bos(onizleme);
            const f = dosyaG.files && dosyaG.files[0];
            if (!f) return;
            try {
                hazir = await antetHazirla(f);
                if (!adG.value.trim()) adG.value = f.name.replace(/\.[^.]+$/, '');
                onizleme.appendChild(antetResmi(Object.assign({ ad: adG.value }, hazir), 600));
                onizleme.appendChild(e('div', { sinif: 'alt', stil: 'margin-top:4px', metin: 'Belgede ' + hazir.genislik + ' × ' + hazir.yukseklik + ' pt olarak yer alacak.' }));
            } catch (x) { hata.textContent = x.message; }
        });
        const ekle = async function() {
            hata.textContent = '';
            if (!hazir) { hata.textContent = 'Önce bir resim seçin.'; return; }
            try {
                await istek('antet_ekle', { ad: adG.value, resim: hazir.resim, genislik: hazir.genislik, yukseklik: hazir.yukseklik, varsayilan: varsG.checked });
                adG.value = ''; dosyaG.value = ''; varsG.checked = false; hazir = null; bos(onizleme);
                bildir('Antet kaydedildi.');
                yukle();
            } catch (x) { hata.textContent = x.message; }
        };
        kap.appendChild(e('div', { sinif: 'a-kart' }, [
            e('h3', { metin: 'Yeni antet' }),
            e('div', { sinif: 'a-form' }, [
                e('label', null, ['Resim (PNG, JPEG…)', dosyaG]),
                e('label', null, ['Ad', adG]),
                e('button', { sinif: 'a-d ana', type: 'button', onclick: ekle, metin: 'Kaydet' })
            ]),
            e('label', { stil: 'display:flex;flex-direction:row;align-items:center;gap:6px;margin-top:10px;font-weight:400' }, [varsG, 'Varsayılan antet olsun (yeni belgeler bununla açılır)']),
            onizleme,
            hata
        ]));
        const liste = e('div', null, [e('div', { sinif: 'a-bilgi', metin: 'Yükleniyor…' })]);
        kap.appendChild(liste);

        async function yukle() {
            let l;
            try { l = await istek('antetler'); } catch (x) { bos(liste).appendChild(e('div', { sinif: 'a-hata', metin: x.message })); return; }
            l = Array.isArray(l) ? l : [];
            sayiYaz('antet', l.length);
            bos(liste);
            if (!l.length) { liste.appendChild(e('div', { sinif: 'a-bilgi', metin: 'Henüz antet kaydedilmedi. Yukarıdan bir resim seçin.' })); return; }
            const islem = async function(uc, govde, mesaj) {
                hata.textContent = '';
                try { await istek(uc, govde); if (mesaj) bildir(mesaj); yukle(); } catch (y) { hata.textContent = y.message; }
            };
            l.forEach(function(x) {
                const ad = e('span', { sinif: 'a-etiket', metin: x.ad, title: 'Adı değiştirmek için tıklayın', tabindex: '0' });
                const duzenle = function() {
                    const i = e('input', { type: 'text', value: x.ad || '', maxlength: '120', stil: 'max-width:320px' });
                    let bitti = false;
                    const kaydet = function() { if (bitti) return; bitti = true; islem('antet_ad', { id: x.id, ad: i.value }, 'Antet adı kaydedildi.'); };
                    i.addEventListener('keydown', function(ev) {
                        if (ev.key === 'Enter') { ev.preventDefault(); kaydet(); }
                        else if (ev.key === 'Escape') { ev.stopPropagation(); bitti = true; yukle(); }
                    });
                    i.addEventListener('blur', kaydet);
                    ad.replaceWith(i);
                    i.focus();
                };
                ad.addEventListener('click', duzenle);
                ad.addEventListener('keydown', function(ev) { if (ev.key === 'Enter') duzenle(); });
                liste.appendChild(e('div', { sinif: 'a-kart', stil: x.varsayilan ? 'border-color:var(--a-vurgu)' : '' }, [
                    e('div', { stil: 'display:flex;align-items:center;gap:10px;flex-wrap:wrap;margin-bottom:10px' }, [
                        e('strong', null, [ad]),
                        x.varsayilan ? e('span', { sinif: 'a-rozet', metin: 'Varsayılan' }) : null,
                        e('span', { sinif: 'alt', metin: Math.round(x.genislik) + ' × ' + Math.round(x.yukseklik) + ' pt' }),
                        e('div', { sinif: 'a-bosluk' }),
                        e('div', { sinif: 'a-eylem' }, [
                            x.varsayilan
                                ? e('button', { sinif: 'a-d kucuk', type: 'button', metin: 'Varsayılanı kaldır', title: 'Yeni belgeler antetsiz açılır',
                                    onclick: function() { islem('antet_varsayilan', { id: null }, 'Varsayılan antet kaldırıldı.'); } })
                                : e('button', { sinif: 'a-d kucuk', type: 'button', metin: 'Varsayılan yap',
                                    onclick: function() { islem('antet_varsayilan', { id: x.id }, 'Varsayılan antet değişti.'); } }),
                            silDugmesi(function() { islem('antet_sil', { id: x.id }, 'Antet silindi.'); })
                        ])
                    ]),
                    antetResmi(x, 860)
                ]));
            });
        }
        yukle();
    }

    // ── Bağlantılar ──
    document.addEventListener('click', function(ev) {
        if (gorunum !== 'acik') return;
        const a = ev.target.closest && ev.target.closest('#av-sidebar-wrapper a, #av-sidebar-wrapper button.sideLink');
        if (a && !a.closest('#cu-menu-ayarlar') && !a.closest('.fav-menu-icon')) kucult();
    }, true);
    window.addEventListener('resize', yerlestir);
    window.__cuAyarlarAc = ac;
    setInterval(function() { dugmeKur(); menuGirisi(); yerlestir(); gorevGuncelle(false); }, 700);
    dugmeKur();
    menuGirisi();
    const v = oku(ACIK_ANAHTAR, '');
    if (v === '1') setTimeout(ac, 300);
    else if (v === 'k') { gorunum = 'kucuk'; setTimeout(function() { gorevGuncelle(true); }, 300); }
})();
