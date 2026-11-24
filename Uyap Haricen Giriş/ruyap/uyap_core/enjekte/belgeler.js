// ═══════════════════════════════════════════════════════════════════════════════════════
// ÇOKLU UYAP — BÜRO BELGELERİ SAYFASI (2026-11-24; plan docs/claude-notlar/planlar/yazi-cokluuyap.md)
// ═══════════════════════════════════════════════════════════════════════════════════════
// Armonia Yazı'da büroya kaydedilen belgeler. Ofis diskinde yargı türü → birim → dosya klasörlerinde durur
// (src/belge_disk.rs); sayfa aynı ağacı gösterir. Veri ofis bilgisayarındaki büro veritabanından
// (/__cu__/veri/is/belge_agac, belge_liste…; src/is_api.rs). UYAP'a istek atılmaz, buluta veri gitmez.
//
// İş Listesi / İcra Takip Süreçleri gibi pencere davranır: sol menüde "Büro Belgeleri", görev çubuğunda kendi
// düğmesi (kaynak 'belgeler'), küçült / öne getir; öbür sayfalardan biri açılınca bu küçülür.
// Solda klasör ağacı, sağda liste; arama; Belgelerim / Tümü. Eylemler: Aç (Armonia Yazı), İndir, Taşı (başka dosyaya
// ya da dosyasız), Yeniden adlandır, Sil (kalıcı). Telefonda klasörler listenin üstünde açılır kapanır.
(function() {
    'use strict';
    const anaCerceve = window.__uyapAnaCerceve ? window.__uyapAnaCerceve() : window.top === window;
    if (!anaCerceve || window.__cuBelgeler) return;
    window.__cuBelgeler = true;

    const UC = '/__cu__/veri/is/';
    const ACIK_ANAHTAR = 'cuBelgelerAcik';
    const TUMU_ANAHTAR = 'cuBelgelerTumu';

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
    function oku(k, v) { try { const x = localStorage.getItem(k); return x === null ? v : x; } catch (y) { return v; } }
    function yaz(k, v) { try { localStorage.setItem(k, v); } catch (y) {} }

    async function yanit(r) {
        let j = null;
        try { j = await r.json(); } catch (x) {}
        if (!j) throw new Error('Ofis bilgisayarından beklenmeyen yanıt (' + r.status + '). Program güncel olmayabilir.');
        if (!j.ok) {
            const h = new Error(j.kod === 'E-402' ? 'Büro veritabanı hazır değil (E-402). Ofis bilgisayarında program açık olmalı.'
                : (j.error || 'İşlem yapılamadı.'));
            h.kod = j.kod;
            throw h;
        }
        return j.veri;
    }
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
        return yanit(r);
    }
    function sorgu(o) {
        const q = new URLSearchParams();
        Object.keys(o).forEach(function(k) { if (o[k] !== '' && o[k] != null) q.set(k, o[k]); });
        const s = q.toString();
        return s ? '?' + s : '';
    }

    // ── Stil (İş Listesi ile aynı tema değişkenleri) ──
    function stilKur() {
        if (document.getElementById('cu-belgeler-stil')) return;
        const css = [
            '#cu-belgeler{--b-zemin:var(--cu-t-zemin,#f3f4f7);--b-yuzey:var(--cu-t-yuzey,#fff);--b-serit:var(--cu-t-serit,#f5f6fa);',
            '--b-kenar:var(--cu-t-kenar,#dee2e6);--b-yazi:var(--cu-t-yazi,#212529);--b-vurgu:var(--cu-vurgu,#2d4596);',
            '--b-vurgu-koyu:var(--cu-vurgu-koyu,#1f3277);--b-vurgu-zemin:var(--cu-vurgu-zemin,#e8ecf8);',
            '--b-soluk:color-mix(in srgb,var(--b-yazi) 74%,var(--b-yuzey));--b-silik:color-mix(in srgb,var(--b-yazi) 64%,var(--b-yuzey));',
            'position:fixed;z-index:1400;top:var(--cu-nav-yuk,60px);right:0;bottom:var(--cu-cubuk-yuk,44px);left:0;',
            'background:var(--b-zemin);color:var(--b-yazi);font:14px/1.45 "Segoe UI",system-ui,sans-serif;display:flex;flex-direction:column;overflow:hidden}',
            'html[data-cu-tema="gunes"] #cu-belgeler{--b-vurgu-koyu:var(--cu-vurgu-yazi,#6b4a00)}',
            'html[data-cu-tema="koyu"] #cu-belgeler{--b-zemin:#1d2130;--b-yuzey:#2a2f40;--b-serit:#262b3b;--b-kenar:#3a4055;--b-yazi:#dedede;',
            '--b-vurgu:#3a55b5;--b-vurgu-koyu:#8ea2e6;--b-vurgu-zemin:#2c3554}',
            '#cu-belgeler *{box-sizing:border-box}',
            '#cu-belgeler button,#cu-belgeler input{font:inherit;color:inherit}',
            '#cu-belgeler .b-bas{display:flex;align-items:center;gap:14px;padding:14px 22px 12px;flex-wrap:wrap;border-bottom:1px solid var(--b-kenar)}',
            '#cu-belgeler .b-bas h1{font-size:19px;margin:0;font-weight:600}',
            '#cu-belgeler .b-durum{color:var(--b-soluk);font-size:12.5px}',
            '#cu-belgeler .b-bosluk{flex:1}',
            '#cu-belgeler .b-d{border:1px solid var(--b-kenar);background:var(--b-yuzey);border-radius:9px;padding:7px 13px;cursor:pointer;white-space:nowrap}',
            '#cu-belgeler .b-d:hover{border-color:var(--b-vurgu)}',
            '#cu-belgeler .b-d.ana{background:var(--b-vurgu);border-color:var(--b-vurgu);color:var(--cu-vurgu-uzeri,#fff)}',
            '#cu-belgeler .b-d.kucuk{padding:4px 9px;font-size:12.5px;border-radius:8px}',
            '#cu-belgeler .b-d.tehlike{color:#b84444}',
            '#cu-belgeler .b-d.tehlike.ana{background:#b84444;border-color:#b84444;color:#fff}',
            '#cu-belgeler .b-d[disabled]{opacity:.55;cursor:default}',
            '#cu-belgeler .b-govde{flex:1;display:flex;min-height:0}',
            '#cu-belgeler .b-agac{width:300px;flex:none;overflow:auto;border-right:1px solid var(--b-kenar);background:var(--b-yuzey);padding:10px 8px}',
            '#cu-belgeler .b-dugum{display:flex;align-items:center;gap:4px;width:100%;border:0;background:none;text-align:left;padding:5px 6px;border-radius:7px;cursor:pointer}',
            '#cu-belgeler .b-dugum:hover{background:var(--b-serit)}',
            '#cu-belgeler .b-dugum.sec{background:var(--b-vurgu-zemin);color:var(--b-vurgu-koyu);font-weight:600}',
            '#cu-belgeler .b-ok{width:16px;flex:none;color:var(--b-silik);font-size:10px;text-align:center}',
            '#cu-belgeler .b-ad{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}',
            '#cu-belgeler .b-sayi{color:var(--b-silik);font-size:12px;font-weight:400}',
            '#cu-belgeler .b-ana{flex:1;overflow:auto;padding:14px 22px 22px;min-width:0}',
            '#cu-belgeler .b-arac{display:flex;gap:10px;align-items:center;margin-bottom:12px;flex-wrap:wrap}',
            '#cu-belgeler input[type=search],#cu-belgeler input[type=text]{background:var(--b-yuzey);border:1px solid var(--b-kenar);border-radius:9px;padding:7px 10px;min-width:0}',
            '#cu-belgeler input[type=search]{flex:1;max-width:420px}',
            '#cu-belgeler .b-secim{display:inline-flex;border:1px solid var(--b-kenar);border-radius:9px;overflow:hidden}',
            '#cu-belgeler .b-secim button{border:0;background:var(--b-yuzey);padding:6px 12px;cursor:pointer}',
            '#cu-belgeler .b-secim button.on{background:var(--b-vurgu);color:var(--cu-vurgu-uzeri,#fff)}',
            '#cu-belgeler .b-yol{color:var(--b-soluk);font-size:12.5px;margin:-4px 0 10px;word-break:break-word}',
            '#cu-belgeler table{width:100%;border-collapse:separate;border-spacing:0;background:var(--b-yuzey);border:1px solid var(--b-kenar);border-radius:12px;overflow:hidden}',
            '#cu-belgeler th{text-align:left;font-weight:600;font-size:12px;color:var(--b-soluk);background:var(--b-serit);padding:8px 12px;white-space:nowrap}',
            '#cu-belgeler td{padding:8px 12px;border-top:1px solid var(--b-kenar);vertical-align:top}',
            '#cu-belgeler tr.b-satir:hover td{background:var(--b-serit)}',
            '#cu-belgeler .b-belge-ad{font-weight:600;word-break:break-word;cursor:pointer}',
            '#cu-belgeler .b-belge-ad:hover{text-decoration:underline}',
            '#cu-belgeler .alt{color:var(--b-soluk);font-size:12.5px}',
            '#cu-belgeler .chip{display:inline-block;padding:1px 8px;border-radius:999px;font-size:12px;font-weight:500;white-space:nowrap;margin:2px 4px 0 0;',
            'background:color-mix(in srgb,var(--r,#8a877e) 16%,var(--b-yuzey));color:color-mix(in srgb,var(--r,#8a877e) 80%,var(--b-yazi))}',
            '#cu-belgeler .b-eylem{display:flex;gap:6px;justify-content:flex-end;flex-wrap:wrap}',
            '#cu-belgeler .b-bilgi{background:var(--b-yuzey);border:1px dashed var(--b-kenar);border-radius:12px;padding:22px;color:var(--b-soluk);text-align:center}',
            '#cu-belgeler .b-hata{background:color-mix(in srgb,#b84444 12%,var(--b-yuzey));color:color-mix(in srgb,#b84444 80%,var(--b-yazi));border-radius:10px;padding:9px 13px;margin-bottom:12px}',
            '#cu-belgeler .b-menu{position:fixed;z-index:1500;display:flex;flex-direction:column;min-width:190px;padding:4px;border-radius:10px;background:var(--b-yuzey);border:1px solid var(--b-kenar);box-shadow:0 10px 28px rgba(0,0,0,.18)}',
            '#cu-belgeler .b-menu button{border:0;background:none;text-align:left;padding:7px 10px;border-radius:7px;cursor:pointer}',
            '#cu-belgeler .b-menu button:hover{background:var(--b-serit)}',
            '#cu-belgeler .b-menu button.tehlike{color:#b84444}',
            '#cu-belgeler .b-katman{position:absolute;inset:0;z-index:5;display:flex;align-items:flex-start;justify-content:center;padding:60px 16px;background:rgba(0,0,0,.28)}',
            '#cu-belgeler .b-kutu{width:460px;max-width:100%;max-height:calc(100% - 40px);overflow:auto;background:var(--b-yuzey);border:1px solid var(--b-kenar);border-radius:14px;padding:16px 18px;box-shadow:0 18px 40px rgba(0,0,0,.25)}',
            '#cu-belgeler .b-kutu h2{font-size:16px;margin:0 0 10px}',
            '#cu-belgeler .b-kutu input[type=text],#cu-belgeler .b-kutu input[type=search]{width:100%;max-width:none}',
            '#cu-belgeler .b-kutu .b-alt{display:flex;gap:8px;justify-content:flex-end;margin-top:14px}',
            '#cu-belgeler .b-kutu .b-not{color:var(--b-soluk);font-size:12.5px;margin-top:8px}',
            '#cu-belgeler .b-sonuc{display:flex;flex-direction:column;gap:4px;margin-top:8px;max-height:280px;overflow:auto}',
            '#cu-belgeler .b-sonuc button{border:1px solid var(--b-kenar);background:var(--b-yuzey);border-radius:9px;padding:7px 10px;text-align:left;cursor:pointer}',
            '#cu-belgeler .b-sonuc button:hover{border-color:var(--b-vurgu)}',
            '#cu-belgeler .b-klasor-dugme{display:none}',
            '#cu-menu-belgeler .SidebarLabel{font-weight:600}',
            '#cu-menu-belgeler.on > li > a{background:var(--cu-vurgu-zemin,#e8ecf8)}',
            // Dar ekran: klasörler listenin üstünde açılır/kapanır, tablo kartlara döner
            'html.cu-dar #cu-belgeler .b-bas{padding:10px 12px}',
            'html.cu-dar #cu-belgeler .b-govde{flex-direction:column}',
            'html.cu-dar #cu-belgeler .b-agac{width:auto;max-height:45%;border-right:0;border-bottom:1px solid var(--b-kenar);display:none}',
            'html.cu-dar #cu-belgeler.agac-acik .b-agac{display:block}',
            'html.cu-dar #cu-belgeler .b-klasor-dugme{display:inline-block}',
            'html.cu-dar #cu-belgeler .b-ana{padding:12px}',
            'html.cu-dar #cu-belgeler table,html.cu-dar #cu-belgeler tbody,html.cu-dar #cu-belgeler tr,html.cu-dar #cu-belgeler td{display:block;width:100%}',
            'html.cu-dar #cu-belgeler thead{display:none}',
            'html.cu-dar #cu-belgeler tr.b-satir{border-top:1px solid var(--b-kenar);padding:6px 0}',
            'html.cu-dar #cu-belgeler tr.b-satir:first-child{border-top:0}',
            'html.cu-dar #cu-belgeler td{border-top:0;padding:3px 12px}',
            'html.cu-dar #cu-belgeler .b-eylem{justify-content:flex-start}',
            'html.cu-dar #cu-belgeler td.b-bilgi-h{display:inline-block;width:auto;padding-right:0}',
            'html.cu-dar #cu-belgeler td.b-bilgi-h::before{content:attr(data-b) " "}',
            'html.cu-dar #cu-belgeler td.b-bilgi-h + td.b-bilgi-h::before{content:"· " attr(data-b) " "}',
            'html.cu-dar #cu-belgeler .b-arac input[type=search]{order:-1;flex:1 1 100%;max-width:none}',
            'html.cu-dar #cu-belgeler .b-bas h1{font-size:17px}',
        ].join('\n');
        document.head.appendChild(e('style', { id: 'cu-belgeler-stil', metin: css }));
    }

    // ── Durum ──
    const d = {
        agac: null, liste: [], klasor: '', tumu: oku(TUMU_ANAHTAR, '1') === '1', ara: '', hata: '',
        acik: new Set(), yukleniyor: false
    };
    let kok = null, agacEl = null, anaEl = null, araZaman = null, gorunum = 'kapali', istekNo = 0;

    function durumSakla() {
        try {
            if (gorunum === 'kapali') sessionStorage.removeItem(ACIK_ANAHTAR);
            else sessionStorage.setItem(ACIK_ANAHTAR, gorunum === 'acik' ? '1' : 'k');
        } catch (x) {}
    }

    // ── Menü girişi (İş Listesi'nin altında) ──
    function menuGirisi() {
        const yan = document.getElementById('av-sidebar-wrapper');
        if (!yan || document.getElementById('cu-menu-belgeler')) return;
        const ust = yan.querySelector('.sidebar-top-part') || yan.firstElementChild || yan;
        const ul = e('ul', { id: 'cu-menu-belgeler', sinif: 'menu-list' + (gorunum === 'acik' ? ' on' : '') }, [
            e('li', { sinif: 'SidebarLink' }, [
                e('a', { href: '#', title: 'Büroda kaydedilen belgeler: yargı türü, birim ve dosya klasörlerinde',
                         onclick: function(ev) { ev.preventDefault(); ac(true); } },
                  [e('span', { sinif: 'SidebarLabel', metin: 'Büro Belgeleri' })])
            ])
        ]);
        const isler = document.getElementById('cu-menu-isler');
        if (isler && isler.parentNode === ust) ust.insertBefore(ul, isler.nextSibling); else ust.insertBefore(ul, ust.firstChild);
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
    sayfalar['belgeler'] = function() { if (gorunum === 'acik') kucult(); };
    function digerSayfalariIndir() {
        Object.keys(sayfalar).forEach(function(k) { if (k !== 'belgeler') try { sayfalar[k](); } catch (x) {} });
    }
    function ac() {
        stilKur();
        gorunum = 'acik';
        durumSakla();
        if (!kok) {
            kok = e('div', { id: 'cu-belgeler', role: 'region', 'aria-label': 'Büro Belgeleri' });
            document.body.appendChild(kok);
            ciz();
            yukle();
        }
        digerSayfalariIndir();
        kok.style.display = '';
        const m = document.getElementById('cu-menu-belgeler');
        if (m) m.classList.add('on');
        yerlestir();
        gorevGuncelle(true);
    }
    function kucult() {
        if (gorunum === 'kapali') return;
        gorunum = 'kucuk';
        durumSakla();
        menuKapat();
        if (kok) kok.style.display = 'none';
        const m = document.getElementById('cu-menu-belgeler');
        if (m) m.classList.remove('on');
        gorevGuncelle(true);
    }
    function kapat() {
        gorunum = 'kapali';
        durumSakla();
        menuKapat();
        if (kok) kok.remove();
        kok = null; agacEl = null; anaEl = null;
        const m = document.getElementById('cu-menu-belgeler');
        if (m) m.classList.remove('on');
        gorevGuncelle(true);
    }
    let gorevImza = null;
    function gorevTikla() { if (gorunum === 'acik') kucult(); else ac(); }
    function gorevGuncelle(zorla) {
        if (!window.CU || !window.CU.gorev) return;
        const imza = gorunum;
        if (!zorla && imza === gorevImza) return;
        gorevImza = imza;
        if (gorunum === 'kapali') { window.CU.gorev.ayarla([], 'belgeler'); return; }
        window.CU.gorev.ayarla([{
            kimlik: 'belgeler', ad: 'Büro Belgeleri', ikon: '📁', on: gorunum === 'acik', kucuk: gorunum !== 'acik',
            tikla: gorevTikla, kapat: kapat,
            menu: function() { return [[[gorunum === 'acik' ? 'Küçült' : 'Öne getir', gorevTikla]]]; },
        }], 'belgeler');
    }

    // ── Çizim ──
    function ciz() {
        if (!kok) return;
        bos(kok);
        kok.appendChild(e('div', { sinif: 'b-bas' }, [
            e('h1', { metin: 'Büro Belgeleri' }), e('div', { sinif: 'b-durum', id: 'cu-belgeler-durum' }), e('div', { sinif: 'b-bosluk' }),
            e('button', { sinif: 'b-d ana', metin: '+ Yeni belge', title: 'Armonia Yazı\'da yeni belge (seçili dosyanın klasörüne kaydedilir)', onclick: yeniBelge }),
            e('button', { sinif: 'b-d', metin: '–', title: 'Görev çubuğuna küçült', 'aria-label': 'Küçült', stil: 'min-width:38px;font-weight:700', onclick: kucult }),
            e('button', { sinif: 'b-d', metin: 'Kapat', title: 'Sayfayı kapat (görev çubuğundan da çıkar)', onclick: kapat })
        ]));
        agacEl = e('nav', { sinif: 'b-agac', 'aria-label': 'Klasörler' });
        anaEl = e('div', { sinif: 'b-ana' });
        kok.appendChild(e('div', { sinif: 'b-govde' }, [agacEl, anaEl]));
        agacCiz();
        anaCiz();
    }
    function durumYaz() {
        const el = document.getElementById('cu-belgeler-durum');
        if (!el) return;
        const n = d.agac ? d.agac.sayi : 0;
        el.textContent = d.agac ? n + ' belge' + (d.tumu ? '' : ' (benim)') : '';
    }

    // Klasör adının gösterimi: dosya klasörü "2026-123" → "2026/123" (diskte "/" kullanılamaz)
    function dugumAdi(dg, derinlik) {
        if (derinlik === 2 && /^\d+-\d+/.test(dg.ad)) return dg.ad.replace(/^(\d+)-(\d+)/, '$1/$2');
        return dg.ad;
    }
    function yolGosterimi(k) {
        return k.split('/').filter(Boolean).map(function(p, i) { return dugumAdi({ ad: p }, i); }).join(' › ');
    }
    function agacCiz() {
        if (!agacEl) return;
        bos(agacEl);
        const kokSayi = d.agac ? d.agac.sayi : 0;
        agacEl.appendChild(dugumDugme({ ad: d.tumu ? 'Tüm belgeler' : 'Belgelerim', klasor: '', sayi: kokSayi, alt: [] }, 0, true));
        if (!d.agac) { agacEl.appendChild(e('div', { sinif: 'alt', stil: 'padding:8px', metin: 'Yükleniyor…' })); return; }
        if (!d.agac.alt.length) agacEl.appendChild(e('div', { sinif: 'alt', stil: 'padding:8px 10px', metin: 'Henüz büroya kaydedilmiş belge yok. Armonia Yazı\'da "Büroya kaydet" ile kaydedilen belgeler burada dosyalarına göre klasörlenir.' }));
        (function dal(liste, derinlik) {
            liste.forEach(function(dg) {
                agacEl.appendChild(dugumDugme(dg, derinlik + 1, false));
                if (dg.alt.length && d.acik.has(dg.klasor)) dal(dg.alt, derinlik + 1);
            });
        })(d.agac.alt, 0);
    }
    function dugumDugme(dg, derinlik, kokMu) {
        const acilir = !kokMu && dg.alt && dg.alt.length > 0;
        const acik = d.acik.has(dg.klasor);
        return e('button', {
            sinif: 'b-dugum' + (d.klasor === dg.klasor ? ' sec' : ''), title: kokMu ? '' : dg.klasor.split('/').join(' › '),
            stil: 'padding-left:' + (6 + Math.max(0, derinlik - 1) * 16) + 'px',
            onclick: function() {
                if (acilir) { if (acik && d.klasor === dg.klasor) d.acik.delete(dg.klasor); else d.acik.add(dg.klasor); }
                klasorSec(dg.klasor);
            }
        }, [
            e('span', { sinif: 'b-ok', metin: acilir ? (acik ? '▾' : '▸') : (kokMu ? '' : '·') }),
            e('span', { sinif: 'b-ad', metin: kokMu ? dg.ad : dugumAdi(dg, derinlik - 1) }),
            e('span', { sinif: 'b-sayi', metin: String(dg.sayi) })
        ]);
    }
    function klasorSec(k) {
        d.klasor = k;
        // Seçilen klasörün üstleri açık kalsın
        const p = k.split('/');
        for (let i = 1; i < p.length; i++) d.acik.add(p.slice(0, i).join('/'));
        if (kok) kok.classList.remove('agac-acik');
        agacCiz();
        listeYukle();
    }

    function anaCiz() {
        if (!anaEl) return;
        bos(anaEl);
        const ara = e('input', { type: 'search', placeholder: 'Belge adı, birim ya da dosya no ara…', value: d.ara, 'aria-label': 'Belge ara' });
        ara.addEventListener('input', function() {
            clearTimeout(araZaman);
            araZaman = setTimeout(function() { d.ara = ara.value.trim(); listeYukle(); }, 300);
        });
        const secim = e('div', { sinif: 'b-secim', role: 'group', 'aria-label': 'Kapsam' }, [
            e('button', { sinif: d.tumu ? '' : 'on', metin: 'Belgelerim', title: 'Oluşturduğunuz ya da işleriniz yoluyla bağlı olduğunuz belgeler',
                          onclick: function() { kapsam(false); } }),
            e('button', { sinif: d.tumu ? 'on' : '', metin: 'Tümü', title: 'Görmeye yetkili olduğunuz bütün büro belgeleri', onclick: function() { kapsam(true); } })
        ]);
        anaEl.appendChild(e('div', { sinif: 'b-arac' }, [
            e('button', { sinif: 'b-d b-klasor-dugme', metin: '📁 Klasörler', onclick: function() { kok.classList.toggle('agac-acik'); } }),
            ara, secim,
            e('button', { sinif: 'b-d', metin: 'Yenile', title: 'Klasörleri ve listeyi yeniden yükle', onclick: yukle })
        ]));
        if (d.klasor) anaEl.appendChild(e('div', { sinif: 'b-yol', metin: '📁 ' + yolGosterimi(d.klasor) }));
        if (d.hata) anaEl.appendChild(e('div', { sinif: 'b-hata', metin: d.hata }));
        const liste = e('div', { id: 'cu-belgeler-liste' });
        anaEl.appendChild(liste);
        listeCiz();
    }
    function kapsam(t) {
        d.tumu = t;
        yaz(TUMU_ANAHTAR, t ? '1' : '0');
        anaCiz();
        yukle();
    }

    function listeCiz() {
        const kutu = document.getElementById('cu-belgeler-liste');
        if (!kutu) return;
        bos(kutu);
        if (d.yukleniyor && !d.liste.length) { kutu.appendChild(e('div', { sinif: 'b-bilgi', metin: 'Yükleniyor…' })); return; }
        if (!d.liste.length) {
            kutu.appendChild(e('div', { sinif: 'b-bilgi', metin: d.ara ? 'Aramaya uyan belge yok.' : (d.klasor ? 'Bu klasörde belge yok.' : 'Belge yok.') }));
            return;
        }
        const tbody = e('tbody');
        d.liste.forEach(function(b) { tbody.appendChild(satir(b)); });
        kutu.appendChild(e('table', {}, [
            e('thead', {}, [e('tr', {}, ['Belge', 'Sürüm', 'Son kayıt', 'Oluşturan', ''].map(function(t) { return e('th', { metin: t }); }))]),
            tbody
        ]));
        if (d.liste.length >= 200) kutu.appendChild(e('div', { sinif: 'alt', stil: 'margin-top:8px', metin: 'İlk 200 belge gösteriliyor; aramayla ya da klasör seçerek daraltın.' }));
    }
    // Belgenin, seçili klasöre göre alt yolu (dosya klasöründeyken boş)
    function altYol(b) {
        const k = b.klasor || '';
        if (d.klasor === k) return '';
        const tam = yolGosterimi(k).split(' › ');
        const atla = d.klasor && k.indexOf(d.klasor + '/') === 0 ? d.klasor.split('/').length : 0;
        return tam.slice(atla).join(' › ');
    }
    function satir(b) {
        const yol = altYol(b);
        const cipler = [];
        if (b.imzali) cipler.push(e('span', { sinif: 'chip', stil: '--r:#4f8a57', metin: 'e-imzalı' }));
        if (b.gonderim) cipler.push(e('span', { sinif: 'chip', stil: '--r:#3f7fa3', title: (b.gonderim.tur_adi || '') + ' · ' + (b.gonderim.zaman || '') + ' · ' + (b.gonderim.kisi || ''), metin: 'UYAP\'a gönderildi' }));
        if (b.is_sayisi) cipler.push(e('span', { sinif: 'chip', stil: '--r:#7a5fb0', metin: b.is_sayisi + ' işte' }));
        if (b.bicim === 'docx') cipler.push(e('span', { sinif: 'chip', metin: 'Word' }));
        const menuDugme = e('button', { sinif: 'b-d kucuk', metin: '⋯', title: 'Diğer işlemler', 'aria-label': 'Diğer işlemler',
                                         onclick: function(ev) { ev.stopPropagation(); satirMenusu(ev.currentTarget, b); } });
        return e('tr', { sinif: 'b-satir' }, [
            e('td', {}, [
                e('div', { sinif: 'b-belge-ad', metin: b.ad, title: 'Armonia Yazı\'da aç', onclick: function() { belgeAc(b); } }),
                yol ? e('div', { sinif: 'alt', metin: '📁 ' + yol }) : null,
                cipler.length ? e('div', {}, cipler) : null
            ]),
            e('td', { sinif: 'alt b-bilgi-h', 'data-b': 'Sürüm', metin: String(b.son_surum) }),
            e('td', { sinif: 'alt b-bilgi-h', 'data-b': '', metin: b.guncelleme || '' }),
            e('td', { sinif: 'alt b-bilgi-h', 'data-b': '', metin: b.olusturan || '' }),
            e('td', {}, [e('div', { sinif: 'b-eylem' }, [
                e('button', { sinif: 'b-d kucuk', metin: 'Aç', title: 'Armonia Yazı\'da aç', disabled: !window.__cuYaziBelgeAc, onclick: function() { belgeAc(b); } }),
                menuDugme
            ])])
        ]);
    }

    // ── Satır menüsü ──
    let menuEl = null;
    function menuKapat() { if (menuEl) { menuEl.remove(); menuEl = null; } }
    function satirMenusu(dugme, b) {
        menuKapat();
        const l = [
            ['İndir', function() { indir(b); }],
            ['Taşı (başka dosyaya)…', function() { tasiPenceresi(b); }],
            ['Yeniden adlandır…', function() { adPenceresi(b); }],
            ['Sil…', function() { silPenceresi(b); }, 'tehlike']
        ];
        menuEl = e('div', { sinif: 'b-menu', role: 'menu' }, l.map(function(x) {
            return e('button', { role: 'menuitem', sinif: x[2] || '', metin: x[0], onclick: function() { menuKapat(); x[1](); } });
        }));
        kok.appendChild(menuEl);
        const r = dugme.getBoundingClientRect(), mw = menuEl.offsetWidth, mh = menuEl.offsetHeight;
        menuEl.style.left = Math.max(8, Math.min(r.right - mw, window.innerWidth - mw - 8)) + 'px';
        menuEl.style.top = (r.bottom + mh + 8 > window.innerHeight ? Math.max(8, r.top - mh - 4) : r.bottom + 4) + 'px';
    }
    document.addEventListener('pointerdown', function(ev) { if (menuEl && !menuEl.contains(ev.target)) menuKapat(); }, true);

    // ── Pencereler (sayfanın içinde; tarayıcı onay kutusu kullanılmaz) ──
    function kutuAc(baslik) {
        const katman = e('div', { sinif: 'b-katman' });
        const kutu = e('div', { sinif: 'b-kutu', role: 'dialog', 'aria-label': baslik }, [e('h2', { metin: baslik })]);
        katman.appendChild(kutu);
        function kapa() { katman.remove(); document.removeEventListener('keydown', tus, true); }
        function tus(ev) { if (ev.key === 'Escape') { ev.stopPropagation(); kapa(); } }
        katman.addEventListener('pointerdown', function(ev) { if (ev.target === katman) kapa(); });
        document.addEventListener('keydown', tus, true);
        kok.appendChild(katman);
        return { kutu: kutu, kapa: kapa };
    }
    function altDugmeler(k, anaMetin, anaSinif, is) {
        const durum = e('div', { sinif: 'b-not', role: 'status' });
        const vazgec = e('button', { sinif: 'b-d', metin: 'Vazgeç', onclick: k.kapa });
        const ana = e('button', { sinif: 'b-d ana ' + (anaSinif || ''), metin: anaMetin });
        ana.addEventListener('click', async function() {
            ana.disabled = vazgec.disabled = true;
            try { await is(); k.kapa(); }
            catch (x) { durum.textContent = x && x.message || String(x); ana.disabled = vazgec.disabled = false; }
        });
        k.kutu.append(durum, e('div', { sinif: 'b-alt' }, [vazgec, ana]));
        return ana;
    }
    function adPenceresi(b) {
        const k = kutuAc('Yeniden adlandır');
        const girdi = e('input', { type: 'text', value: b.ad, maxlength: '200', 'aria-label': 'Yeni ad' });
        k.kutu.append(girdi, e('div', { sinif: 'b-not', metin: 'Dosya klasöründeki adı da değişir. Aynı klasörde aynı adda belge varsa sonuna (2) eklenir.' }));
        const ana = altDugmeler(k, 'Kaydet', '', async function() {
            const r = await istek('belge_adlandir', { id: b.id, ad: girdi.value });
            bildir('Yeni ad: ' + r.ad);
            yukle();
        });
        girdi.addEventListener('keydown', function(ev) { if (ev.key === 'Enter') ana.click(); });
        setTimeout(function() { girdi.focus(); girdi.select(); }, 0);
    }
    function silPenceresi(b) {
        const k = kutuAc('Belgeyi sil');
        k.kutu.append(e('div', { metin: '"' + b.ad + '" bütün sürümleriyle (' + b.son_surum + ') KALICI olarak silinecek; dosya klasöründen ve bağlı olduğu işlerden de çıkar. Geri alınamaz.' }));
        altDugmeler(k, 'Kalıcı olarak sil', 'tehlike', async function() {
            await istek('belge_sil', { id: b.id });
            bildir('Silindi: ' + b.ad);
            yukle();
        });
    }
    function tasiPenceresi(b) {
        const k = kutuAc('Belgeyi taşı');
        k.kutu.append(e('div', { sinif: 'b-not', stil: 'margin:0 0 8px', metin: 'Şu an: ' + (b.dosya_no ? (b.birim_adi + ' ' + b.dosya_no) : 'dosyasız') + '. Büro veritabanındaki bir dosyayı seçin; belge eski sürümleriyle o dosyanın klasörüne taşınır.' }));
        const ara = e('input', { type: 'search', placeholder: 'Dosya no, birim ya da taraf adı (en az 2 harf)', 'aria-label': 'Dosya ara' });
        const sonuc = e('div', { sinif: 'b-sonuc' });
        const durum = e('div', { sinif: 'b-not', role: 'status' });
        k.kutu.append(ara, sonuc, durum);
        let z = null, no = 0;
        async function tasi(dosyaId, ad) {
            durum.textContent = 'Taşınıyor…';
            try {
                await istek('belge_dosya', { id: b.id, dosya_id: dosyaId });
                k.kapa();
                bildir('Taşındı: ' + b.ad + ' → ' + ad);
                yukle();
            } catch (x) { durum.textContent = x.message; }
        }
        ara.addEventListener('input', function() {
            clearTimeout(z);
            z = setTimeout(async function() {
                const n = ++no, q = ara.value.trim();
                bos(sonuc);
                if (q.length < 2) return;
                try {
                    const l = await istek('dosya_ara' + sorgu({ ara: q }));
                    if (n !== no) return;
                    if (!l.length) { sonuc.appendChild(e('div', { sinif: 'alt', metin: 'Büro veritabanında uyan dosya yok.' })); return; }
                    l.forEach(function(ds) {
                        const ad = ds.birim_adi + ' ' + ds.dosya_no;
                        sonuc.appendChild(e('button', { disabled: ds.id === b.dosya_id, onclick: function() { tasi(ds.id, ad); } }, [
                            e('div', { metin: ad + (ds.id === b.dosya_id ? ' (şu anki)' : '') }),
                            ds.borclu ? e('div', { sinif: 'alt', metin: ds.borclu }) : null
                        ]));
                    });
                } catch (x) { if (n === no) durum.textContent = x.message; }
            }, 300);
        });
        const alt = e('div', { sinif: 'b-alt' }, [
            b.dosya_id || b.dosya_no ? e('button', { sinif: 'b-d', metin: 'Dosyadan ayır (dosyasız)', title: 'Belge Dosyasız/<oluşturan> klasörüne taşınır',
                                                      onclick: function() { tasi(null, 'Dosyasız'); } }) : null,
            e('button', { sinif: 'b-d', metin: 'Vazgeç', onclick: k.kapa })
        ]);
        k.kutu.append(alt);
        setTimeout(function() { ara.focus(); }, 0);
    }

    // ── Eylemler ──
    function belgeAc(b) {
        if (!window.__cuYaziBelgeAc) { bildir('Armonia Yazı bu kurulumda yok.'); return; }
        window.__cuYaziBelgeAc(b.id);
    }
    async function indir(b) {
        try {
            const r = await fetch(UC + 'belge_indir' + sorgu({ id: b.id }), { credentials: 'same-origin', cache: 'no-store' });
            if (!r.ok) await yanit(r);
            const url = URL.createObjectURL(await r.blob());
            const a = e('a', { href: url, download: b.ad + '.' + (b.bicim || 'udf') });
            document.body.appendChild(a);
            a.click();
            setTimeout(function() { URL.revokeObjectURL(url); a.remove(); }, 3000);
        } catch (x) { bildir('İndirilemedi: ' + (x && x.message || x)); }
    }
    // Yeni belge: seçili klasör bir dosyanınsa (listede o dosyanın belgesi varsa) editör o dosyaya bağlanır
    function yeniBelge() {
        if (!window.__cuYaziYeni) { bildir('Armonia Yazı bu kurulumda yok.'); return; }
        const p = d.klasor.split('/');
        const ds = p.length === 3 && p[0] !== 'Dosyasız' ? d.liste.find(function(b) { return b.klasor === d.klasor && b.dosya_id; }) : null;
        window.__cuYaziYeni(ds ? { buroDosyaId: ds.dosya_id, birimAdi: ds.birim_adi, dosyaNo: ds.dosya_no } : {});
    }

    // ── Yükleme ──
    async function yukle() {
        const n = ++istekNo;
        d.hata = '';
        try {
            const a = await istek('belge_agac' + sorgu({ tumu: d.tumu ? '1' : '0' }));
            if (n !== istekNo) return;
            d.agac = a;
            // Seçili klasör artık yoksa (son belgesi taşındı/silindi) en yakın üst klasöre
            while (d.klasor && !klasorVar(d.klasor)) d.klasor = d.klasor.split('/').slice(0, -1).join('/');
        } catch (x) {
            if (n !== istekNo) return;
            d.hata = x.message;
        }
        durumYaz();
        agacCiz();
        await listeYukle();
    }
    function klasorVar(k) {
        let l = d.agac ? d.agac.alt : [];
        const p = k.split('/');
        for (let i = 0; i < p.length; i++) {
            const yol = p.slice(0, i + 1).join('/');
            const dg = l.find(function(x) { return x.klasor === yol; });
            if (!dg) return false;
            l = dg.alt;
        }
        return true;
    }
    async function listeYukle() {
        const n = ++istekNo;
        d.yukleniyor = true;
        listeCiz();
        try {
            const l = await istek('belge_liste' + sorgu({ klasor: d.klasor, ara: d.ara, tumu: d.tumu ? '1' : '0' }));
            if (n !== istekNo) return;
            d.liste = l;
            if (d.hata && anaEl) { d.hata = ''; anaCiz(); return; }
        } catch (x) {
            if (n !== istekNo) return;
            d.liste = [];
            if (d.hata !== x.message) { d.hata = x.message; d.yukleniyor = false; anaCiz(); return; }
        }
        d.yukleniyor = false;
        const yol = anaEl && anaEl.querySelector('.b-yol');
        if (anaEl && (!!yol !== !!d.klasor || (yol && yol.textContent !== '📁 ' + yolGosterimi(d.klasor)))) { anaCiz(); return; }
        listeCiz();
    }

    // ── Bağlantılar ──
    // Editörde kaydedilen / silinen belge: sayfa açıksa tazelenir (açık değilse bir sonraki açılışta)
    let tazeZaman = null;
    window.addEventListener('cu-belge-degisti', function() {
        if (!kok) return;
        clearTimeout(tazeZaman);
        tazeZaman = setTimeout(yukle, 400);
    });
    document.addEventListener('click', function(ev) {
        if (gorunum !== 'acik') return;
        const a = ev.target.closest && ev.target.closest('#av-sidebar-wrapper a, #av-sidebar-wrapper button.sideLink');
        if (a && !a.closest('#cu-menu-belgeler') && !a.closest('.fav-menu-icon')) kucult();
    }, true);
    document.addEventListener('keydown', function(ev) {
        if (ev.key === 'Escape' && menuEl) menuKapat();
    });
    window.addEventListener('resize', function() { yerlestir(); menuKapat(); });
    setInterval(function() { menuGirisi(); yerlestir(); gorevGuncelle(false); }, 700);
    menuGirisi();
    try {
        const v = sessionStorage.getItem(ACIK_ANAHTAR);
        if (v === '1') setTimeout(function() { ac(); }, 300);
        else if (v === 'k') { gorunum = 'kucuk'; setTimeout(function() { gorevGuncelle(true); }, 300); }
    } catch (x) {}
    // Başka ekranlardan: window.__cuBelgelerAc(klasor?)
    window.__cuBelgelerAc = function(klasor) {
        ac();
        if (typeof klasor === 'string') klasorSec(klasor);
    };
})();
