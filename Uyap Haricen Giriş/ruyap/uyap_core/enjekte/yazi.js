// ═══════════════════════════════════════════════════════════════════════════════════════
// ÇOKLU UYAP — ARMONIA YAZI PENCERESİ (2026-11-23)
// ═══════════════════════════════════════════════════════════════════════════════════════
// Armonia Yazı editörü (ayrı depoda geliştiriliyor; derlenmiş hâli uyap_core/yazi, sunucu src/yazi_sunucu.rs)
// UYAP ekranının içinde, modal OLMAYAN bir pencerede açılır: UYAP'ta açık belge okunurken yanında yazılır
// (kullanıcı isteği 23.11.2026). Varsayılan yerleşim sağ yarı ("yan yana"); ⇆ ile serbest pencereye geçer
// (taşınır, boyutlandırılır). Telefonda tam ekran.
// Editör aynı adresten (/__cu__/yazi/) çerçevede açıldığı için kodunu değiştirmeden:
//   • çoklu pano: çerçevede Alt+1…0 (ya da Ctrl+1…0) yapıştırır, Alt+Shift+1…0 seçimi panoya kaydeder (aynı gözler);
//   • tema: ÇokluUYAP renkleri editörün değişkenlerine aktarılır;
//   • sayfa ölçeği ilk açılışta "sayfa genişliği" (editör dar pencerede zaten kendisi sığdırır);
//   • belge açma: window.__cuYaziDosyaAc(File) belgeyi editöre sürükle-bırak gibi verir (paylas.js "Yazı'da aç").
// Kaydetme şimdilik editörün kendi "Kaydet"i (indirir); veritabanına kayıt editör köprüsüyle (2. aşama) gelecek.
(function() {
    'use strict';
    const anaCerceve = window.__uyapAnaCerceve ? window.__uyapAnaCerceve() : window.top === window;
    if (!anaCerceve || window.__cuYazi) return;
    window.__cuYazi = true;

    const ADRES = '/__cu__/yazi/';
    const AYAR = 'cuYaziPencere';
    let kok = null, cerceve = null, baslikAd = null, durum = 'kapali', bekleyen = null, hazir = false, izci = null;
    let ayar = (function() {
        try { return Object.assign({ kip: 'yan', gen: 0, x: 80, y: 80, w: 760, h: 620 }, JSON.parse(localStorage.getItem(AYAR) || '{}')); }
        catch (x) { return { kip: 'yan', gen: 0, x: 80, y: 80, w: 760, h: 620 }; }
    })();
    function ayarYaz() { try { localStorage.setItem(AYAR, JSON.stringify(ayar)); } catch (x) {} }
    const mobil = function() { return document.documentElement.classList.contains('cu-mobil'); };
    function el(tag, sinif, metin) { const e = document.createElement(tag); if (sinif) e.className = sinif; if (metin != null) e.textContent = metin; return e; }
    function bildir(m) { try { if (window.__uyapBildirim) window.__uyapBildirim(m); } catch (x) {} }

    function stilKur() {
        if (document.getElementById('cu-yazi-stil')) return;
        const s = el('style');
        s.id = 'cu-yazi-stil';
        s.textContent = [
            // UYAP'ın açılır pencerelerinin (DevExtreme, z ≥ 1503) üstünde; görev çubuğuyla üst çubuğun arasında.
            '#cu-yazi{position:fixed;z-index:1700;display:flex;flex-direction:column;background:var(--cu-t-yuzey,#fff);color:var(--cu-t-yazi,#222);',
            'border:1px solid var(--cu-t-kenar,#d9dde3);box-shadow:-8px 0 28px rgba(0,0,0,.16);font:13px/1.3 "Segoe UI",system-ui,sans-serif;overflow:hidden}',
            '#cu-yazi.yan{top:var(--cu-nav-yuk,48px);bottom:var(--cu-cubuk-yuk,44px);right:0;border-width:0 0 0 1px}',
            '#cu-yazi.serbest{border-radius:12px;resize:both;min-width:360px;min-height:300px;box-shadow:0 18px 50px rgba(0,0,0,.28)}',
            'html.cu-mobil #cu-yazi{top:var(--cu-nav-yuk,48px)!important;bottom:var(--cu-cubuk-yuk,44px)!important;left:0!important;right:0!important;width:auto!important;height:auto!important;border-radius:0;resize:none}',
            '#cu-yazi .cu-yz-bas{display:flex;align-items:center;gap:6px;height:36px;padding:0 6px 0 12px;flex:none;',
            'background:var(--cu-t-serit,#f5f6fa);border-bottom:1px solid var(--cu-t-kenar,#d9dde3);user-select:none}',
            '#cu-yazi.serbest .cu-yz-bas{cursor:move}',
            '#cu-yazi .cu-yz-ad{flex:1;min-width:0;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}',
            '#cu-yazi .cu-yz-ad b{font-weight:600;margin-right:6px;flex:none}',
            '#cu-yazi .cu-yz-ad{display:flex;align-items:center}',
            '#cu-yazi .cu-yz-adm{min-width:0;overflow:hidden;text-overflow:ellipsis}',
            '#cu-yazi .cu-yz-durum{flex:none}',
            '#cu-yazi .cu-yz-bas button{border:0;background:none;color:inherit;font:inherit;height:28px;min-width:30px;padding:0 8px;border-radius:7px;cursor:pointer}',
            '#cu-yazi .cu-yz-bas button:hover{background:var(--cu-vurgu-zemin,#e8ecf8)}',
            '#cu-yazi .cu-yz-bas button.ana{background:var(--cu-vurgu,#2d4596);color:var(--cu-vurgu-uzeri,#fff)}',
            '#cu-yazi .cu-yz-uyari{color:var(--cu-t-yazi,#222);font-weight:600}',
            '#cu-yazi iframe{flex:1;width:100%;border:0;background:var(--cu-t-zemin,#f3f4f7)}',
            '#cu-yazi.surukleniyor iframe{pointer-events:none}',
            '#cu-yazi .cu-yz-tutamac{position:absolute;left:0;top:0;bottom:0;width:6px;cursor:ew-resize;z-index:1}',
            '#cu-yazi.serbest .cu-yz-tutamac,#cu-yazi.tam .cu-yz-tutamac,html.cu-mobil #cu-yazi .cu-yz-tutamac{display:none}',
            'html.cu-mobil #cu-yazi .cu-yz-kip,html.cu-mobil #cu-yazi .cu-yz-buyut{display:none}',
            '#cu-yazi .cu-yz-bas button:disabled{opacity:.5;cursor:default}',
            '#cu-yazi .cu-yz-bas button.ana:disabled{background:var(--cu-t-kenar,#d9dde3);color:var(--cu-t-yazi,#222)}',
            '#cu-yazi .cu-yz-bag{display:inline-flex;align-items:center;gap:2px;max-width:34%;padding:0 2px 0 8px;height:24px;border-radius:12px;',
            'background:var(--cu-vurgu-zemin,#e8ecf8);font-size:12px;white-space:nowrap;overflow:hidden}',
            '#cu-yazi .cu-yz-bag>span{overflow:hidden;text-overflow:ellipsis}',
            '#cu-yazi .cu-yz-bag button{height:20px;min-width:20px;padding:0 4px}',
            '#cu-yazi .cu-yz-bag[hidden],#cu-yazi [hidden]{display:none!important}',
            '#cu-yazi .cu-yz-menu{position:absolute;z-index:3;display:flex;flex-direction:column;min-width:170px;padding:4px;border-radius:10px;',
            'background:var(--cu-t-yuzey,#fff);border:1px solid var(--cu-t-kenar,#d9dde3);box-shadow:0 10px 28px rgba(0,0,0,.18)}',
            '#cu-yazi .cu-yz-menu button{border:0;background:none;color:inherit;font:inherit;text-align:left;padding:7px 10px;border-radius:7px;cursor:pointer}',
            '#cu-yazi .cu-yz-menu button:hover{background:var(--cu-vurgu-zemin,#e8ecf8)}',
            '#cu-yazi .cu-yz-katman{position:absolute;inset:36px 0 0 0;z-index:4;display:flex;align-items:flex-start;justify-content:center;padding:40px 16px;background:rgba(0,0,0,.28)}',
            '#cu-yazi .cu-yz-kutu{width:min(460px,100%);padding:16px 18px;border-radius:12px;background:var(--cu-t-yuzey,#fff);color:var(--cu-t-yazi,#222);',
            'border:1px solid var(--cu-t-kenar,#d9dde3);box-shadow:0 18px 50px rgba(0,0,0,.28)}',
            '#cu-yazi .cu-yz-kutu h3{margin:0 0 10px;font-size:15px;font-weight:600}',
            '#cu-yazi .cu-yz-satir{display:block;margin:0 0 10px}',
            '#cu-yazi .cu-yz-satir select,#cu-yazi .cu-yz-satir input{display:block;width:100%;margin-top:4px;height:32px;padding:0 8px;box-sizing:border-box;',
            'border:1px solid var(--cu-t-kenar,#d9dde3);border-radius:8px;background:var(--cu-t-zemin,#fff);color:inherit;font:inherit}',
            '#cu-yazi .cu-yz-not{min-height:18px;margin:4px 0 10px;font-weight:600}',
            '#cu-yazi .cu-yz-alt{display:flex;justify-content:flex-end;gap:8px}',
            '#cu-yazi .cu-yz-alt button{border:1px solid var(--cu-t-kenar,#d9dde3);background:none;color:inherit;font:inherit;height:32px;padding:0 14px;border-radius:8px;cursor:pointer}',
            '#cu-yazi .cu-yz-alt button.ana{background:var(--cu-vurgu,#2d4596);border-color:transparent;color:var(--cu-vurgu-uzeri,#fff)}',
            '#cu-yazi .cu-yz-alt button:disabled{opacity:.5;cursor:default}',
            '#cu-yazi .cu-yz-durum{margin-left:8px;padding:1px 7px;border-radius:9px;font-size:11px;background:var(--cu-t-kenar,#d9dde3)}',
            '#cu-yazi .cu-yz-durum.kayitli{background:var(--cu-vurgu-zemin,#e8ecf8)}',
            '#cu-yazi .cu-yz-bas button.cu-yz-ok{min-width:22px;padding:0 4px;margin-left:-6px}',
            '#cu-yazi .cu-yz-menu hr{border:0;border-top:1px solid var(--cu-t-kenar,#d9dde3);margin:4px 2px}',
            '#cu-yazi .cu-yz-kutu.genis{width:min(620px,100%)}',
            '#cu-yazi .cu-yz-kutu input:not([type]){display:block;width:100%;height:32px;padding:0 8px;box-sizing:border-box;margin:4px 0 8px;',
            'border:1px solid var(--cu-t-kenar,#d9dde3);border-radius:8px;background:var(--cu-t-zemin,#fff);color:inherit;font:inherit}',
            '#cu-yazi .cu-yz-liste{display:flex;flex-direction:column;gap:2px;max-height:220px;overflow:auto;margin-bottom:8px}',
            '#cu-yazi .cu-yz-liste.uzun{max-height:min(52vh,420px)}',
            '#cu-yazi .cu-yz-oge{display:flex;flex-direction:column;align-items:flex-start;gap:2px;border:0;background:none;color:inherit;font:inherit;',
            'text-align:left;padding:7px 10px;border-radius:8px;cursor:pointer}',
            '#cu-yazi .cu-yz-oge:hover{background:var(--cu-vurgu-zemin,#e8ecf8)}',
            '#cu-yazi .cu-yz-oge span,#cu-yazi .cu-yz-soluk,#cu-yazi .cu-yz-bos{font-size:12px;opacity:.75}',
            '#cu-yazi .cu-yz-bos{padding:8px 10px}',
            '#cu-yazi .cu-yz-sekmeler{display:flex;gap:6px;margin-bottom:6px;flex-wrap:wrap}',
            '#cu-yazi .cu-yz-sekmeler button,#cu-yazi .cu-yz-bag-degistir{border:1px solid var(--cu-t-kenar,#d9dde3);background:none;color:inherit;font:inherit;',
            'height:28px;padding:0 10px;border-radius:14px;cursor:pointer}',
            '#cu-yazi .cu-yz-sekmeler button.secili{background:var(--cu-vurgu,#2d4596);border-color:transparent;color:var(--cu-vurgu-uzeri,#fff)}',
            '#cu-yazi .cu-yz-bag-degistir{margin:-4px 0 8px}',
            '#cu-yazi .cu-yz-oge.secili{background:var(--cu-vurgu-zemin,#e8ecf8);outline:2px solid var(--cu-vurgu,#2d4596)}',
        ].join('\n');
        document.head.appendChild(s);
    }

    function yerlestir() {
        if (!kok) return;
        kok.classList.toggle('yan', ayar.kip === 'yan');
        kok.classList.toggle('serbest', ayar.kip !== 'yan');
        kok.classList.toggle('tam', ayar.kip === 'yan' && !!ayar.tam);
        if (mobil()) return;
        const vw = window.innerWidth, vh = window.innerHeight;
        if (ayar.kip === 'yan') {
            // Büyütülmüşse bütün genişlik (UYAP kartı arkada kalır); değilse sağ taraf, kart solda (kartlar.js).
            const gen = ayar.tam ? vw : Math.round(Math.min(Math.max(ayar.gen || vw / 2, 380), vw - 160));
            Object.assign(kok.style, { width: gen + 'px', left: '', height: '', top: '', bottom: '' });
        } else {
            const w = Math.min(Math.max(ayar.w, 360), vw - 20), h = Math.min(Math.max(ayar.h, 300), vh - 20);
            const x = Math.min(Math.max(ayar.x, 0), vw - 120), y = Math.min(Math.max(ayar.y, 0), vh - 60);
            Object.assign(kok.style, { left: x + 'px', top: y + 'px', width: w + 'px', height: h + 'px', bottom: '' });
        }
    }

    // ── Çerçevedeki editörle köprü (aynı köken) ──
    function cw() { try { return cerceve && cerceve.contentWindow; } catch (x) { return null; } }
    function cd() { try { return cerceve && cerceve.contentDocument; } catch (x) { return null; } }
    function degisti() {
        const d = cd(), du = d && d.getElementById('durum');
        return !!(du && /•\s*$/.test(du.textContent || ''));
    }
    function belgeAdi() {
        const d = cd();
        const t = d ? String(d.title || '') : '';
        return /—\s*Armonia Yazı$/.test(t) ? t.replace(/\s*—\s*Armonia Yazı$/, '') : '';
    }
    const TEMA = [['--zemin', '--cu-t-zemin'], ['--cubuk', '--cu-t-yuzey'], ['--kenar', '--cu-t-kenar'], ['--yazi', '--cu-t-yazi'],
                  ['--vurgu', '--cu-vurgu'], ['--vurgu-koyu', '--cu-vurgu-koyu'], ['--vurgu-zemin', '--cu-vurgu-zemin']];
    function temaAktar() {
        const d = cd();
        if (!d || !d.documentElement) return;
        const cs = getComputedStyle(document.documentElement);
        TEMA.forEach(function(t) {
            const v = cs.getPropertyValue(t[1]).trim();
            if (v) d.documentElement.style.setProperty(t[0], v); else d.documentElement.style.removeProperty(t[0]);
        });
    }
    // Çoklu pano (uyap_proxy'deki gözler, window.__uyapCokluPano): metin YAZILIR gibi eklenir (satır satır insertText,
    // aralarda Enter). Böylece imlecin bulunduğu yerin yazı tipi / boyutu ve paragraf biçimi korunur; editörün
    // "yapıştır"ı düz metne varsayılan biçimi veriyordu (23.11 denemesi). Geri al (Ctrl+Z) çalışır.
    // Kısayol kuralı programdakiyle aynı (window.__uyapPanoKisayolu): Ctrl+N ya da Alt+N (Chrome Ctrl+1…9'u sekme
    // geçişine ayırır); Shift ile kaydeder.
    function panoTusu(ev) {
        const p = window.__uyapCokluPano, kural = window.__uyapPanoKisayolu;
        if (!p || !kural) return;
        const n = kural(ev);
        if (!n) return;
        const m = [n, n];
        const i = (Number(n) + 9) % 10;               // '1'→0 … '9'→8, '0'→9 (PANO_SIRA)
        const w = cw(), d = cd();
        if (!w || !d) return;
        ev.preventDefault();
        ev.stopPropagation();
        if (ev.shiftKey) { p.kaydet(i, String(w.getSelection ? w.getSelection() : '')); return; }
        const metin = p.oku()[i];
        if (!metin) { bildir('Pano ' + m[1] + ' boş (kaydetmek için metni seçip Alt+Shift+' + m[1] + ').'); return; }
        const hedef = d.activeElement && d.activeElement.isContentEditable ? d.activeElement : d.querySelector('[contenteditable="true"]');
        if (!hedef) return;
        hedef.focus();
        const satirlar = String(metin).replace(/\r\n?/g, '\n').split('\n');
        satirlar.forEach(function(s, j) {
            if (j > 0) {
                const ke = { key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true, cancelable: true };
                const kd = new w.KeyboardEvent('keydown', ke);
                if (hedef.dispatchEvent(kd)) { try { d.execCommand('insertParagraph'); } catch (x) {} }
            }
            if (s) { try { d.execCommand('insertText', false, s); } catch (x) {} }
        });
    }
    function dosyaVer(f) {
        const w = cw();
        if (!w || !hazir) { bekleyen = f; return; }
        try {
            const dt = new w.DataTransfer();
            dt.items.add(new w.File([f], f.name, { type: f.type || 'application/octet-stream' }));
            bizimVerme = true;
            try { w.dispatchEvent(new w.DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true })); }
            finally { bizimVerme = false; }
        } catch (x) { bildir('Belge editöre verilemedi: ' + (x && x.message || x)); }
    }
    // ── E-imza (24.11.2026) ──
    // Editörün "İmzala"sı yalnız Armonia masaüstünde çalışıyor. ÇokluUYAP'ta aynı düğme programın imza hizmetine
    // (/__uyap_agent__/sign_udf: ofis bilgisayarına takılı kart, girişte verilen PIN) bağlanır. Kullanıcı kararı:
    // şimdilik YALNIZ ofis bilgisayarında; uzaktan (cokluuyap.com) bağlananda imza yok.
    // Belgenin son hâli editörün kendi "Kaydet"inden alınır: çerçevedeki indirme bağlantısı yakalanır, dosya
    // indirilmez (editörde köprü API'si yok; bkz. docs/claude-notlar/planlar/yazi-cokluuyap.md).
    function buBilgisayar() { return /^(127\.0\.0\.1|localhost|\[::1\]|::1)$/i.test(location.hostname); }
    let kayitYakala = null, imzaSuruyor = false;
    function yakalamaKur(w) {
        const bloblar = new Map();
        const asilUrl = w.URL.createObjectURL;
        w.URL.createObjectURL = function(o) {
            const u = asilUrl.apply(w.URL, arguments);
            if (o instanceof w.Blob) { bloblar.set(u, o); if (bloblar.size > 5) bloblar.delete(bloblar.keys().next().value); }
            return u;
        };
        const asilTik = w.HTMLAnchorElement.prototype.click;
        w.HTMLAnchorElement.prototype.click = function() {
            const b = kayitYakala && this.hasAttribute('download') ? bloblar.get(this.href) : null;
            if (b) { const y = kayitYakala; kayitYakala = null; y(new File([b], this.download || 'belge.udf')); return; }
            return asilTik.apply(this, arguments);
        };
    }
    function dugmeBul(d, ad) {
        return Array.from(d.querySelectorAll('#arac-cubugu button')).find(function(b) { return (b.textContent || '').trim() === ad; });
    }
    // Editörün "Farklı kaydet" listesi (UDF / DOCX): kaydet(biçim) imzalı (salt okunur) belgede de çalışır.
    function farkliKaydet(d, bicim) {
        const s = d.querySelector('select[title="Farklı kaydet"]');
        const w = cw();
        if (!s || !w || !Array.from(s.options).some(function(o) { return o.value === bicim; })) return false;
        s.value = bicim;
        s.dispatchEvent(new w.Event('change', { bubbles: true }));
        return true;
    }
    // Belgenin son hâli (varsayılan UDF), indirilmeden yakalanır.
    function sonHali(d, bicim) {
        return new Promise(function(coz, red) {
            const zaman = setTimeout(function() { kayitYakala = null; red(new Error('Belge editörden alınamadı.')); }, 8000);
            kayitYakala = function(f) { clearTimeout(zaman); coz(f); };
            if (farkliKaydet(d, bicim || 'udf')) return;
            const k = dugmeBul(d, 'Kaydet');
            if (!k) { clearTimeout(zaman); kayitYakala = null; red(new Error('Editörün Kaydet düğmesi bulunamadı.')); return; }
            bizimTik = true;
            try { k.click(); } finally { bizimTik = false; }
        });
    }
    function indir(f) {
        const a = document.createElement('a');
        a.href = URL.createObjectURL(f);
        a.download = f.name;
        a.setAttribute('data-uyap-internal', '1');
        document.body.appendChild(a); a.click(); a.remove();
        setTimeout(function() { URL.revokeObjectURL(a.href); }, 30000);
    }
    function kok_ad(ad) { return String(ad || 'belge').replace(/\.(udf|docx|pdf)$/i, ''); }
    async function imzala() {
        const d = cd();
        if (!d || imzaSuruyor) return;
        if (!buBilgisayar()) { bildir('E-imza şimdilik yalnız ofis bilgisayarında (kartın takılı olduğu yerde) kullanılabilir.'); return; }
        if (!belgeAdi()) { bildir('Önce bir belge açın.'); return; }
        // Editör imzalı belgeyi salt okunur açar ve #bildirim'i "imza" şeridi yapar ("Düzenlemeyi aç" gizler)
        const serit = d.getElementById('bildirim');
        if (serit && !serit.hidden && serit.classList.contains('imza')) { bildir('Belge zaten e-imzalı.'); return; }
        imzaSuruyor = true;
        try {
            const f = await sonHali(d);
            if (!/\.(udf|docx)$/i.test(f.name)) throw new Error('Yalnız UDF ve Word belgesi imzalanabilir.');
            bildir('E-imzalanıyor… (kart takılı olmalı)');
            const r = await fetch('/__uyap_agent__/sign_udf?name=' + encodeURIComponent(f.name), {
                method: 'POST', body: f, credentials: 'include', cache: 'no-store', headers: { 'Content-Type': 'application/octet-stream' } });
            if (!r.ok) {
                let m = 'İmzalama başarısız (' + r.status + ').';
                try { const j = await r.json(); if (j && j.error) m = j.error; } catch (x) {}
                throw new Error(m);
            }
            const dosya = new File([await r.arrayBuffer()], kok_ad(f.name) + '.udf', { type: 'application/octet-stream' });
            // Kullanıcı isteği (24.11): imzalayınca İNDİRİLMEZ. İmzalı dosya burada tutulur ("Kaydet ▾ → İmzalı UDF",
            // "Dosyaya gönder") ve editörde salt okunur açılır (değişip imza bozulmasın).
            dosyaVer(dosya);
            imzali = dosya;
            imzaliZaman = Date.now();
            araclariGuncelle();
            let ek = '';
            if (kayit) {
                try { await buroyaYaz(dosya, { imzali: true, notu: 'E-imza' }); ek = ' İmzalı sürüm büroya kaydedildi (sürüm ' + kayit.surum + ').'; }
                catch (x) { ek = ' İmzalı sürüm büroya kaydedilemedi: ' + (x && x.message || x); }
            }
            bildir('İmzalandı: ' + dosya.name + '.' + ek + (bagli ? ' Göndermek için "Dosyaya gönder".' : ''));
        } catch (x) {
            bildir('İmzalanamadı: ' + (x && x.message || x));
        } finally { imzaSuruyor = false; }
    }
    function imzaDugmesi(e) {
        const b = e.target && e.target.closest && e.target.closest('#arac-cubugu button');
        if (!b) return;
        const ad = (b.textContent || '').trim();
        if (ad === 'Aç') {
            // Editörün "Aç"ı doğrudan dosya seçici açıyordu: önce büro / bilgisayar seçimi
            e.preventDefault();
            e.stopImmediatePropagation();
            acMenusu(b);
            return;
        }
        if (ad === 'Yeni') { belgeBirak(); return; }   // başka belge: imzalı dosya / büro kaydı ona ait değil
        if (ad === 'Kaydet' && !bizimTik) {
            // Editörün kendi "Kaydet"i de büroya kaydeder (indirme ▾ menüsünde)
            e.preventDefault();
            e.stopImmediatePropagation();
            buroKaydet();
            return;
        }
        if (ad !== 'İmzala') return;
        e.preventDefault();
        e.stopImmediatePropagation();
        imzala();
    }

    // ── İmzalı dosya, kaydetme biçimleri, bağlı dosya ve gönderme (24.11.2026, plan: yazi-cokluuyap.md) ──
    let imzali = null;        // son imzalanan UDF (File); editörde açık olan belge
    let kayit = null;         // editördeki belgenin büro kaydı: { id, surum, ad }
    let bizimTik = false;     // editörün Kaydet'ine kendimiz basıyoruz (büro kaydına çevrilmesin)
    let imzaliZaman = 0;
    let bagli = null;         // bağlı UYAP dosyası: kart verisi (dosyaId, dosyaNo, birimId, birimAdi, dosyaTurKod…)
    let bizimVerme = false;   // dosyaVer'in kendi sürükle-bırakı (kullanıcının açtığı belge değil)
    let araclar = null;       // başlıktaki { bag, kaydet, gonder }
    function imzaliBirak() { if (imzali) { imzali = null; araclariGuncelle(); } }
    function belgeBirak() { imzali = null; kayit = null; yeniIcin = null; araclariGuncelle(); sonBaslik = ''; }
    let yeniIcin = null;      // yeni belge bir iş için yazılıyor: { gorevId } (ilk kayıtta işe eklenir)
    function imzaSeridi() {
        const d = cd(), s = d && d.getElementById('bildirim');
        return !!(s && !s.hidden && s.classList.contains('imza'));
    }
    function dosyaEtiketi(v) { return v ? [v.birimAdi, v.dosyaNo].filter(Boolean).join(' ') : ''; }
    function bagla(v) {
        if (!v || !v.dosyaId) return;
        bagli = { dosyaId: v.dosyaId, dosyaNo: v.dosyaNo, birimId: v.birimId, birimAdi: v.birimAdi, dosyaTurKod: v.dosyaTurKod,
                  dosyaTurKodAciklama: v.dosyaTurKodAciklama, evrakTipi: v.evrakTipi };
        araclariGuncelle();
    }
    function araclariGuncelle() {
        if (!araclar) return;
        araclar.bag.hidden = !bagli;
        if (bagli) {
            araclar.bagAd.textContent = dosyaEtiketi(bagli);
            araclar.bag.title = 'Bağlı dosya: ' + dosyaEtiketi(bagli) + ' (imzalanan belge bu dosyaya gönderilir)';
        }
        // Uzaktan (cokluuyap.com / yerel ağ) bağlanan: imza ve gönderim yalnız ofis bilgisayarında; düğme gösterilmez.
        araclar.gonder.hidden = !bagli || !buBilgisayar();
        const kartli = !!(bagli && bagli.dosyaId);
        araclar.gonder.disabled = !imzali || !kartli;
        araclar.gonder.title = !kartli ? "UYAP'a göndermek için belgeyi UYAP dosya kartından (\"Editörde yaz\" ya da \"Yazı'da aç\") açın"
            : imzali ? 'İmzalı belgeyi ' + dosyaEtiketi(bagli) + ' dosyasına gönder' : 'Önce belgeyi imzalayın (editörde "İmzala")';
    }

    // Kaydet ▾: UDF / Word / PDF; imzalıysa imzalı UDF.
    function menuAc(dugme, girdiler) {
        menuKapat();
        const m = el('div', 'cu-yz-menu');
        girdiler.forEach(function(g) {
            if (!g[1]) { m.appendChild(el('hr')); return; }
            const b = el('button', null, g[0]);
            if (g[2]) b.title = g[2];
            b.addEventListener('click', function() { menuKapat(); g[1](); });
            m.appendChild(b);
        });
        kok.appendChild(m);
        const r = dugme.getBoundingClientRect(), k = kok.getBoundingClientRect();
        m.style.top = (r.bottom - k.top + 2) + 'px';
        m.style.left = Math.max(4, Math.min(r.left - k.left, k.width - m.offsetWidth - 4)) + 'px';
        setTimeout(function() { document.addEventListener('pointerdown', menuDisi, true); }, 0);
    }
    // Editör "Aç" menüsü: düğme çerçevenin içinde; menü üst belgede, çerçevenin konumu eklenerek açılır.
    function acMenusu(b) {
        const f = cerceve;
        const rf = f ? f.getBoundingClientRect() : { top: 0, left: 0 }, rb = b.getBoundingClientRect();
        const konum = { getBoundingClientRect: function() { return { bottom: rf.top + rb.bottom, left: rf.left + rb.left }; } };
        menuAc(konum, [
            ['Büro belgelerini aç…', buroAcPenceresi, 'Büro veritabanındaki belgeler (bu dosyanın / sizin)'],
            ['Bilgisayarımdan aç…', bilgisayardanAc, 'Bu bilgisayardaki bir UDF ya da Word (DOCX) dosyası']
        ]);
        // Çerçevenin içine tıklanınca da kapansın (üst belgenin dinleyicisi çerçeveyi duymaz)
        const d = cd();
        if (d) d.addEventListener('pointerdown', menuKapat, { once: true, capture: true });
    }
    // Dosya seçici üst belgede açılır (tık burada; çerçevedeki seçiciyi kod açamaz), seçilen dosya editöre verilir.
    function bilgisayardanAc() {
        const i = document.createElement('input');
        i.type = 'file';
        i.accept = '.udf,.docx';
        i.style.display = 'none';
        i.addEventListener('change', function() {
            const f = i.files && i.files[0];
            i.remove();
            if (!f) return;
            belgeBirak();
            dosyaVer(f);
        });
        document.body.appendChild(i);
        i.click();
    }
    function menuDisi(e) { if (!e.target.closest || !e.target.closest('.cu-yz-menu')) menuKapat(); }
    function menuKapat() {
        document.removeEventListener('pointerdown', menuDisi, true);
        if (kok) kok.querySelectorAll('.cu-yz-menu').forEach(function(m) { m.remove(); });
    }
    function kaydetMenusu(dugme) {
        const d = cd();
        const g = [['Büro belgelerini aç…', buroAcPenceresi, 'Büro veritabanındaki belgeler (bu dosyanın / sizin)']];
        if (!d || !belgeAdi()) { menuAc(dugme, g); return; }
        g.push(['Büroya yeni belge olarak kaydet…', function() { kaydetPenceresi(true); }, 'Bu belgeyi ayrı bir belge olarak kaydet']);
        if (kayit) {
            g.push(['Onaya sun…', onayPenceresi, 'İş Listesi üzerinden onaya gönder']);
            g.push(['İşe ekle…', isePenceresi, 'Belgeyi İş Listesi\'ndeki bir işe ekle']);
            g.push(['Belgeyi sil…', silPenceresi, 'Belgeyi bütün sürümleriyle büro veritabanından kalıcı olarak sil']);
        }
        g.push(['—', null]);
        if (imzali) g.push(['İndir: İmzalı UDF', function() { indir(imzali); }, 'E-imzalı UDF dosyasını indir']);
        g.push(['İndir: UDF', function() { if (!farkliKaydet(d, 'udf')) bildir('UDF olarak kaydedilemedi.'); },
                imzali ? 'İmzasız UDF (imzalı için "İmzalı UDF")' : 'UYAP belgesi olarak indir']);
        g.push(['İndir: Word (DOCX)', function() { if (!farkliKaydet(d, 'docx')) bildir('Word olarak kaydedilemedi.'); }, 'Word belgesi olarak indir']);
        g.push(['İndir: PDF', pdfKaydet, 'PDF olarak indir']);
        menuAc(dugme, g);
    }
    async function pdfKaydet() {
        const d = cd();
        try {
            const f = imzali || await sonHali(d, 'udf');
            const r = await fetch('/__uyap_yerel__/udf_pdf', { method: 'POST', body: f, credentials: 'include', cache: 'no-store' });
            if (!r.ok) throw new Error('PDF\'e çevrilemedi (' + r.status + ')');
            indir(new File([await r.blob()], kok_ad(f.name) + '.pdf', { type: 'application/pdf' }));
        } catch (x) { bildir('PDF kaydedilemedi: ' + (x && x.message || x)); }
    }

    // Dosyaya gönder: UYAP'ın "Evrak Gönderme" sekmesinin yaptığı (UYAP main.js 24.11.2026):
    //   dosya_gonderilecek_evrak_listesi_brd.ajx {dosyaId} → [[{turKodu, aciklama, …}]] (seçenekler, ada göre sıralı)
    //   odeme_alinacak_evrak_listesi.ajx {dosyaId} → ["CZM_VEKALETNAME", …] (masraf ödemesi isteyen türler)
    //   evrakGonder_brd.ajx (multipart): file_1, items=[{id, evrakTuruOptionDVO, tur, turAciklama, …}], dosyaId,
    //   isDusurme=false, evrakTipi, dosyaNo, birimId, birimAdi  → {type:'success'|…, message}
    // Masraflı türler gönderilmez (UYAP ekranına yönlendirilir). Gönderim yalnız kullanıcının "Gönder"iyle, özet
    // onayından sonra. Önbellek katmanına uğramayan ham fetch (uyap_proxy hamFetch ile aynı yol).
    let hamFetchFn = null;
    function hamFetch(url, init) {
        if (!hamFetchFn) {
            try {
                const f = document.createElement('iframe');
                f.style.display = 'none';
                document.documentElement.appendChild(f);
                hamFetchFn = f.contentWindow.fetch.bind(window);
            } catch (x) { hamFetchFn = window.fetch.bind(window); }
        }
        return hamFetchFn(url, Object.assign({ credentials: 'include', cache: 'no-store' }, init || {}));
    }
    async function uyapJson(uc, govde) {
        const r = await hamFetch('/' + uc, { method: 'POST', body: JSON.stringify(govde),
            headers: { 'Content-Type': 'application/json', 'Accept': 'application/json, text/plain, */*' } });
        if (!r.ok) throw new Error('UYAP ' + uc + ' HTTP ' + r.status);
        return r.json();
    }
    const EK_TURLER = ['.udf', '.pdf', '.jpg', '.png', '.jpeg', '.image/jpg', '.image/jpeg', '.tif', '.tiff', '.UDF', '.JPG', '.JPEG', '.PNG', '.PDF', '.TIF', '.TIFF'];
    async function evrakTurleri(v) {
        if (Number(v.dosyaTurKod) === 294) {
            return [{ tur: 'MTS_BRC_ITIRAZ', label: 'MTS Borcuna İtiraz', mandatory: false, max: 1, ekEvrakMax: 5, sablonTurKodu: '', acceptTypeList: ['.udf', '.UDF'] }];
        }
        const j = await uyapJson('dosya_gonderilecek_evrak_listesi_brd.ajx', { dosyaId: v.dosyaId });
        const s = [];
        (Array.isArray(j) ? j : []).forEach(function(g) {
            (Array.isArray(g) ? g : []).forEach(function(e) {
                if (!e || !e.turKodu) return;
                let kabul = ['.udf', '.UDF'];
                if (e.turKodu === 'CZM_VEKALETNAME' || e.turKodu === 'CZM_YETKI_BELGESI') {
                    kabul = ['.udf', '.pdf', '.jpg', '.jpeg', '.png', '.tif', '.tiff', '.UDF', '.PDF', '.JPG', '.JPEG', '.PNG', '.TIF', '.TIFF'];
                }
                s.push({ tur: e.turKodu, label: e.aciklama || e.turKodu, mandatory: false, max: 1, ekEvrakMax: 5, sablonTurKodu: '', acceptTypeList: kabul });
            });
        });
        s.sort(function(a, b) { return String(a.label).localeCompare(String(b.label), 'tr'); });
        return s;
    }
    async function gonder(v, secenek, aciklama, dosya) {
        const sec = Object.assign({}, secenek);
        if (!sec.ekEvrakAcceptTypeList) sec.ekEvrakAcceptTypeList = EK_TURLER;
        const id = Date.now();
        const item = { id: id, evrakTuruOptionDVO: sec, tur: sec.tur, turAciklama: sec.label, mandatory: sec.mandatory, file: {},
                       path: 'C:/fakepath/' + dosya.name, kullaniciEvrakAciklama: aciklama || '', parentId: -1,
                       isVekalet: false, isVekaletBilgiFormu: false, fileId: 1 };
        const fd = new FormData();
        fd.append('file_1', dosya, dosya.name);
        fd.append('items', JSON.stringify([item]));
        fd.append('dosyaId', v.dosyaId);
        fd.append('isDusurme', false);
        fd.append('evrakTipi', v.evrakTipi);          // UYAP da kart verisindekini yollar (çoğu dosyada boş)
        fd.append('dosyaNo', v.dosyaNo);
        fd.append('birimId', v.birimId);
        fd.append('birimAdi', v.birimAdi);
        const r = await hamFetch('/evrakGonder_brd.ajx', { method: 'POST', body: fd });
        let j = null;
        try { j = await r.json(); } catch (x) {}
        const mesaj = String((j && j.message) || ('HTTP ' + r.status)).replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();
        if (!j || j.type !== 'success') throw new Error(mesaj || 'UYAP gönderimi kabul etmedi.');
        return mesaj;
    }
    function gonderPenceresi() {
        if (!bagli || !bagli.dosyaId || !imzali) return;
        if (!buBilgisayar()) { bildir('Gönderim şimdilik yalnız ofis bilgisayarında.'); return; }
        const v = bagli, dosya = imzali;
        const kat = el('div', 'cu-yz-katman');
        const kutu = el('div', 'cu-yz-kutu');
        kat.appendChild(kutu);
        kat.addEventListener('pointerdown', function(e) { if (e.target === kat) kapa(); });
        function kapa() { kat.remove(); }
        kutu.appendChild(el('h3', null, 'Dosyaya gönder'));
        kutu.appendChild(el('div', 'cu-yz-satir', 'Dosya: ' + dosyaEtiketi(v)));
        kutu.appendChild(el('div', 'cu-yz-satir', 'Belge: ' + dosya.name + ' (e-imzalı)'));
        const etiket = el('label', 'cu-yz-satir', 'Evrak türü');
        const secim = el('select');
        secim.disabled = true;
        secim.appendChild(new Option('Yükleniyor…', ''));
        etiket.appendChild(secim);
        kutu.appendChild(etiket);
        const aEt = el('label', 'cu-yz-satir', 'Açıklama (isteğe bağlı)');
        const aciklama = el('input');
        aciklama.maxLength = 350;
        aEt.appendChild(aciklama);
        kutu.appendChild(aEt);
        const not = el('div', 'cu-yz-not');
        kutu.appendChild(not);
        const alt = el('div', 'cu-yz-alt');
        const vazgec = el('button', null, 'Vazgeç'), git = el('button', 'ana', 'Gönder');
        git.disabled = true;
        alt.append(vazgec, git);
        kutu.appendChild(alt);
        vazgec.addEventListener('click', kapa);
        kok.appendChild(kat);
        let secenekler = [], masrafli = [], onayda = false;
        function secimDegisti() {
            onayda = false;
            git.textContent = 'Gönder';
            const s = secenekler[secim.selectedIndex - 1];
            const m = s && masrafli.indexOf(s.tur) >= 0;
            not.textContent = m ? 'Bu evrak türü masraf ödemesi istiyor; UYAP\'ın "Evrak Gönderme" sekmesinden gönderin.' : '';
            git.disabled = !s || m;
        }
        secim.addEventListener('change', secimDegisti);
        Promise.all([evrakTurleri(v), uyapJson('odeme_alinacak_evrak_listesi.ajx', { dosyaId: v.dosyaId }).catch(function() { return []; })])
            .then(function(s) {
                secenekler = s[0];
                masrafli = Array.isArray(s[1]) ? s[1].map(String) : [];
                secim.replaceChildren(new Option('Evrak türünü seçin…', ''));
                secenekler.forEach(function(x) { secim.appendChild(new Option(x.label, x.tur)); });
                secim.disabled = false;
                secim.focus();
            })
            .catch(function(x) { not.textContent = 'Evrak türleri alınamadı: ' + (x && x.message || x); });
        git.addEventListener('click', async function() {
            const s = secenekler[secim.selectedIndex - 1];
            if (!s) return;
            if (!onayda) {
                // İkinci adım: özet onayı
                onayda = true;
                not.textContent = dosyaEtiketi(v) + ' dosyasına "' + s.label + '" olarak gönderilecek. Onaylıyor musunuz?';
                git.textContent = 'Evet, gönder';
                return;
            }
            git.disabled = true; vazgec.disabled = true; secim.disabled = true; aciklama.disabled = true;
            not.textContent = 'UYAP\'a gönderiliyor…';
            try {
                const m = await gonder(v, s, aciklama.value.trim(), dosya);
                not.textContent = 'Gönderildi. ' + m;
                bildir('Evrak gönderildi (' + dosyaEtiketi(v) + ', ' + s.label + '). ' + m);
                if (kayit) veriIstek('belge_gonderildi', { id: kayit.id, tur: s.tur, tur_adi: s.label, mesaj: m }).catch(function() {});
                alt.replaceChildren(Object.assign(el('button', 'ana', 'Kapat'), { onclick: kapa }));
            } catch (x) {
                not.textContent = 'Gönderilemedi: ' + (x && x.message || x);
                vazgec.disabled = false; secim.disabled = false; aciklama.disabled = false;
                onayda = false; git.textContent = 'Gönder'; git.disabled = false;
            }
        });
    }

    // ── Büro belge deposu (2. aşama B2, 24.11.2026): /__cu__/veri/is/belge_* (belge_veri.rs) ──
    // "Kaydet" (başlık, editörün kendi Kaydet'i, Ctrl+S) büro veritabanına kaydeder; her kayıt yeni sürüm. İlk
    // kayıtta ad ve dosya sorulur (bağlı dosya yoksa büro veritabanında aranır). Arada başkası kaydetmişse sunucu
    // 409 CAKISMA döner: kimsenin yazdığı kaybolmaz.
    const VERI = '/__cu__/veri/is/';
    function b64url(m) {
        const b = new TextEncoder().encode(String(m || ''));
        let s = '';
        b.forEach(function(x) { s += String.fromCharCode(x); });
        return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
    }
    async function veriYanit(r) {
        let j = null;
        try { j = await r.json(); } catch (x) {}
        if (!j) throw new Error('Büro sunucusu yanıt vermedi (' + r.status + ').');
        if (!j.ok) { const e = new Error(j.error || ('Hata ' + r.status)); e.kod = j.kod; throw e; }
        return j.veri;
    }
    async function veriAl(uc, sorgu) {
        const q = new URLSearchParams();
        Object.keys(sorgu || {}).forEach(function(k) { if (sorgu[k] != null && sorgu[k] !== '') q.set(k, sorgu[k]); });
        return veriYanit(await fetch(VERI + uc + (q.toString() ? '?' + q : ''), { credentials: 'include', cache: 'no-store' }));
    }
    async function veriIstek(uc, govde) {
        return veriYanit(await fetch(VERI + uc, { method: 'POST', credentials: 'include', cache: 'no-store',
            headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(govde || {}) }));
    }
    // Bağlı dosyanın büroya yollanacak hâli: UYAP kartı (kart64) ya da büro dosyası (dosya)
    function bagSorgusu() {
        if (!bagli) return {};
        if (bagli.dosyaId) return { kart64: b64url(JSON.stringify(bagli)) };
        if (bagli.buroDosyaId) return { dosya: bagli.buroDosyaId };
        return {};
    }
    // Baytları büroya yazar. o: { yeni, ad, imzali, notu, dosya, gorev }. kayit güncellenir.
    async function buroyaYaz(f, o) {
        o = o || {};
        const q = new URLSearchParams();
        if (kayit && !o.yeni) { q.set('id', kayit.id); q.set('surum', kayit.surum); }
        else {
            q.set('ad64', b64url(o.ad || kok_ad(belgeAdi())));
            const bs = o.dosya ? { dosya: o.dosya } : bagSorgusu();
            Object.keys(bs).forEach(function(k) { q.set(k, bs[k]); });
            const gorev = o.gorev || (yeniIcin && yeniIcin.gorevId);
            if (gorev) q.set('gorev', gorev);
        }
        q.set('bicim', 'udf');
        if (o.imzali) q.set('imzali', '1');
        if (o.notu) q.set('not64', b64url(o.notu));
        const v = await veriYanit(await fetch(VERI + 'belge_kaydet?' + q, { method: 'POST', body: f, credentials: 'include', cache: 'no-store',
            headers: { 'Content-Type': 'application/octet-stream' } }));
        kayit = { id: v.id, surum: v.surum, ad: (kayit && !o.yeni) ? kayit.ad : (o.ad || kok_ad(belgeAdi())) };
        // Editör kaydedince kendi "değişti" bayrağını indirir ama durum yazısını (#durum "•") bir sonraki işleme dek
        // yenilemez; degisti() oradan okuduğu için işaret elle kaldırılır (sonraki yazmada editör yeniden koyar).
        try {
            const du = cd() && cd().getElementById('durum');
            if (du) du.textContent = du.textContent.replace(/\s*•\s*$/, '');
        } catch (x) {}
        sonBaslik = '';
        baslikGuncelle();
        // Büro Belgeleri sayfası (belgeler.js) listeyi tazelesin
        try { window.dispatchEvent(new CustomEvent('cu-belge-degisti', { detail: { id: v.id } })); } catch (x) {}
        return v;
    }
    // Editördeki belgenin kaydedilecek baytları: imzalı ve değişmemişse imzalı dosya, değilse editörün son hâli.
    async function kaydedilecek() {
        const d = cd();
        if (imzali && imzaSeridi()) return { f: imzali, imzali: true };
        return { f: await sonHali(d, 'udf'), imzali: false };
    }
    let kaydediliyor = false;
    async function buroKaydet() {
        if (!belgeAdi()) { bildir('Önce bir belge açın.'); return; }
        if (!kayit) { kaydetPenceresi(false); return; }
        if (kaydediliyor) return;
        kaydediliyor = true;
        try {
            const k = await kaydedilecek();
            const once = kayit.surum;
            const v = await buroyaYaz(k.f, { imzali: k.imzali });
            bildir(v.surum === once ? 'Değişiklik yok; büroda sürüm ' + v.surum + ' duruyor.' : 'Büroya kaydedildi (sürüm ' + v.surum + ').');
        } catch (x) {
            if (x && x.kod === 'CAKISMA') cakismaPenceresi(x.message); else bildir('Kaydedilemedi: ' + (x && x.message || x));
        } finally { kaydediliyor = false; }
    }
    function kutuAc(baslik) {
        const kat = el('div', 'cu-yz-katman');
        const kutu = el('div', 'cu-yz-kutu');
        kat.appendChild(kutu);
        function kapa() { kat.remove(); }
        kat.addEventListener('pointerdown', function(e) { if (e.target === kat) kapa(); });
        kat.addEventListener('keydown', function(e) { if (e.key === 'Escape') kapa(); });
        kutu.appendChild(el('h3', null, baslik));
        kok.appendChild(kat);
        return { kutu: kutu, kapa: kapa };
    }
    // Kalıcı silme: bütün sürümler, iş bağları ve dosyalar (taslaklar birikmesin). Geri alınamaz.
    function silPenceresi() {
        if (!kayit) return;
        const k = kutuAc('Belgeyi sil');
        k.kutu.appendChild(el('div', 'cu-yz-satir', '"' + kayit.ad + '" bütün sürümleriyle (' + kayit.surum + ') büro veritabanından KALICI olarak silinecek; bağlı olduğu işlerden de çıkar. Geri alınamaz.'));
        const durum = el('div', 'cu-yz-satir', '');
        const alt = el('div', 'cu-yz-alt');
        const vazgec = el('button', null, 'Vazgeç'), sil = el('button', 'ana', 'Kalıcı olarak sil');
        vazgec.addEventListener('click', k.kapa);
        sil.addEventListener('click', async function() {
            sil.disabled = vazgec.disabled = true;
            try {
                await veriIstek('belge_sil', { id: kayit.id });
                const ad = kayit.ad;
                kayit = null; sonBaslik = ''; araclariGuncelle();
                try { window.dispatchEvent(new CustomEvent('cu-belge-degisti')); } catch (x) {}
                k.kapa();
                bildir('Silindi: ' + ad + '. Editördeki metin kaydedilmemiş olarak duruyor.');
            } catch (x) { durum.textContent = x && x.message || String(x); sil.disabled = vazgec.disabled = false; }
        });
        alt.append(vazgec, sil);
        k.kutu.append(durum, alt);
    }
    function cakismaPenceresi(m) {
        const k = kutuAc('Belge başkası tarafından değiştirildi');
        k.kutu.appendChild(el('div', 'cu-yz-satir', m));
        const alt = el('div', 'cu-yz-alt');
        const yeni = el('button', null, 'Ayrı belge olarak kaydet'), son = el('button', null, 'Son hâlini aç'), kapat = el('button', 'ana', 'Kapat');
        yeni.addEventListener('click', function() { k.kapa(); kaydetPenceresi(true); });
        son.addEventListener('click', function() { const id = kayit && kayit.id; k.kapa(); if (id) belgeAc({ id: id }); });
        kapat.addEventListener('click', k.kapa);
        alt.append(yeni, son, kapat);
        k.kutu.appendChild(alt);
    }
    // Dosya seçici (büro veritabanı): arama kutusu + sonuçlar. sec(dosya|null) seçimde çağrılır.
    function dosyaSecici(sec) {
        const kap = el('div', 'cu-yz-secici');
        const ara = el('input');
        ara.placeholder = 'Dosya no, birim ya da taraf adıyla arayın (en az 2 harf)';
        const liste = el('div', 'cu-yz-liste');
        kap.append(ara, liste);
        let zaman = 0, sira = 0;
        ara.addEventListener('input', function() {
            clearTimeout(zaman);
            zaman = setTimeout(async function() {
                const n = ++sira, m = ara.value.trim();
                if (m.length < 2) { liste.replaceChildren(); return; }
                try {
                    const s = await veriAl('dosya_ara', { ara: m });
                    if (n !== sira) return;
                    liste.replaceChildren();
                    if (!s.length) liste.appendChild(el('div', 'cu-yz-bos', 'Büro veritabanında bulunamadı.'));
                    s.forEach(function(d) {
                        const b = el('button', 'cu-yz-oge');
                        b.append(el('b', null, d.birim_adi + ' ' + d.dosya_no), el('span', null, d.borclu || d.tur || ''));
                        b.addEventListener('click', function() { sec(d); });
                        liste.appendChild(b);
                    });
                } catch (x) { liste.replaceChildren(el('div', 'cu-yz-bos', 'Aranamadı: ' + (x && x.message || x))); }
            }, 250);
        });
        return { kap: kap, ara: ara };
    }
    // İlk kayıt (ya da "ayrı belge olarak"): ad + dosya.
    function kaydetPenceresi(ayri) {
        if (!belgeAdi()) { bildir('Önce bir belge açın.'); return; }
        const k = kutuAc(ayri ? 'Büroya ayrı belge olarak kaydet' : 'Büroya kaydet');
        const adEt = el('label', 'cu-yz-satir', 'Belge adı');
        const ad = el('input');
        ad.maxLength = 200;
        const onerilen = kok_ad(belgeAdi());
        ad.value = /^yeni belge$/i.test(onerilen) ? '' : onerilen;
        ad.placeholder = 'ör. İtirazın iptali dilekçesi';
        adEt.appendChild(ad);
        k.kutu.appendChild(adEt);
        let secilen = null;   // { id, birim_adi, dosya_no } büro dosyası; null + bagli → bağlı dosya
        const dosyaSatir = el('div', 'cu-yz-satir');
        const secici = dosyaSecici(function(d) { secilen = d; dosyaYaz(); });
        function dosyaYaz() {
            dosyaSatir.replaceChildren(document.createTextNode('Dosya: '));
            if (secilen) dosyaSatir.appendChild(el('b', null, secilen.birim_adi + ' ' + secilen.dosya_no));
            else if (bagli) dosyaSatir.appendChild(el('b', null, dosyaEtiketi(bagli) + ' (bağlı dosya)'));
            else dosyaSatir.appendChild(el('span', 'cu-yz-soluk', 'seçilmedi — aşağıdan arayın ya da dosyasız kaydedin'));
            secici.kap.hidden = !!(secilen || bagli) && !degistir.dataset.acik;
            degistir.hidden = !(secilen || bagli);
        }
        const degistir = el('button', 'cu-yz-bag-degistir', 'Değiştir');
        degistir.addEventListener('click', function() { degistir.dataset.acik = '1'; secici.kap.hidden = false; secici.ara.focus(); });
        k.kutu.append(dosyaSatir, degistir, secici.kap);
        dosyaYaz();
        const not = el('div', 'cu-yz-not');
        k.kutu.appendChild(not);
        const alt = el('div', 'cu-yz-alt');
        const vazgec = el('button', null, 'Vazgeç'), git = el('button', 'ana', 'Kaydet');
        vazgec.addEventListener('click', k.kapa);
        alt.append(vazgec, git);
        k.kutu.appendChild(alt);
        ad.focus();
        git.addEventListener('click', async function() {
            const a = ad.value.trim();
            if (!a) { not.textContent = 'Belge adını yazın.'; ad.focus(); return; }
            git.disabled = vazgec.disabled = true;
            not.textContent = 'Kaydediliyor…';
            try {
                const kv = await kaydedilecek();
                if (ayri) kayit = null;
                const o = { yeni: true, ad: a, imzali: kv.imzali };
                if (secilen) o.dosya = secilen.id;
                const v = await buroyaYaz(kv.f, o);
                // Büro dosyası seçildiyse editör ona bağlanır (UYAP kartı yoksa gönderim kartsızdır)
                if (secilen && !(bagli && bagli.dosyaId && bagli.dosyaNo === secilen.dosya_no)) {
                    bagli = { buroDosyaId: secilen.id, birimAdi: secilen.birim_adi, dosyaNo: secilen.dosya_no };
                    araclariGuncelle();
                }
                k.kapa();
                bildir('Büroya kaydedildi: ' + a + ' (sürüm ' + v.surum + ').');
            } catch (x) {
                not.textContent = 'Kaydedilemedi: ' + (x && x.message || x);
                git.disabled = vazgec.disabled = false;
            }
        });
    }
    // Büro belgelerini aç: bağlı dosyanın belgeleri / benim belgelerim + arama.
    function buroAcPenceresi() {
        const k = kutuAc('Büro belgeleri');
        k.kutu.classList.add('genis');
        const sekmeler = el('div', 'cu-yz-sekmeler');
        const ara = el('input');
        ara.placeholder = 'Belge adı, dosya no ya da birimle arayın';
        const liste = el('div', 'cu-yz-liste uzun');
        let kip = bagli ? 'dosya' : 'benim';
        function sekme(kod, ad) {
            const b = el('button', null, ad);
            b.addEventListener('click', function() { kip = kod; yukle(); });
            b.dataset.kod = kod;
            return b;
        }
        if (bagli) sekmeler.appendChild(sekme('dosya', dosyaEtiketi(bagli)));
        sekmeler.appendChild(sekme('benim', 'Belgelerim'));
        k.kutu.append(sekmeler, ara, liste);
        let zaman = 0, sira = 0;
        async function yukle() {
            sekmeler.querySelectorAll('button').forEach(function(b) { b.classList.toggle('secili', b.dataset.kod === kip); });
            const n = ++sira;
            const q = { ara: ara.value.trim() };
            if (kip === 'dosya') Object.assign(q, bagSorgusu());
            liste.replaceChildren(el('div', 'cu-yz-bos', 'Yükleniyor…'));
            try {
                const s = await veriAl('belge_liste', q);
                if (n !== sira) return;
                liste.replaceChildren();
                if (!s.length) liste.appendChild(el('div', 'cu-yz-bos', kip === 'dosya' ? 'Bu dosyada büroya kaydedilmiş belge yok.' : 'Belge bulunamadı.'));
                s.forEach(function(b) {
                    const o = el('button', 'cu-yz-oge');
                    const ust = [b.dosya_no ? b.birim_adi + ' ' + b.dosya_no : 'dosyasız', 'sürüm ' + b.son_surum, b.guncelleme, b.olusturan];
                    if (b.imzali) ust.push('e-imzalı');
                    if (b.gonderim) ust.push('UYAP\'a gönderildi ' + b.gonderim.zaman);
                    o.append(el('b', null, b.ad), el('span', null, ust.filter(Boolean).join(' · ')));
                    o.addEventListener('click', function() { k.kapa(); belgeAc(b); });
                    liste.appendChild(o);
                });
            } catch (x) { liste.replaceChildren(el('div', 'cu-yz-bos', 'Alınamadı: ' + (x && x.message || x))); }
        }
        ara.addEventListener('input', function() { clearTimeout(zaman); zaman = setTimeout(yukle, 250); });
        const alt = el('div', 'cu-yz-alt');
        const kapat = el('button', null, 'Kapat');
        kapat.addEventListener('click', k.kapa);
        alt.appendChild(kapat);
        k.kutu.appendChild(alt);
        yukle();
        ara.focus();
    }
    // Büro belgesini editörde açar (son sürüm). b: belge_liste satırı ya da { id }.
    async function belgeAc(b) {
        ac();
        try {
            const r = await fetch(VERI + 'belge_indir?id=' + encodeURIComponent(b.id), { credentials: 'include', cache: 'no-store' });
            if (!r.ok) await veriYanit(r);
            const surum = parseInt(r.headers.get('X-Belge-Surum') || '0', 10);
            const imz = r.headers.get('X-Belge-Imzali') === '1';
            let ozet = b;
            if (!ozet.ad) ozet = await veriAl('belge', { id: b.id });
            const f = new File([await r.arrayBuffer()], ozet.ad + '.' + (ozet.bicim || 'udf'), { type: 'application/octet-stream' });
            belgeBirak();
            if (ozet.uyap_karti && ozet.uyap_karti.dosyaId) bagla(ozet.uyap_karti);
            else if (ozet.dosya_id) { bagli = { buroDosyaId: ozet.dosya_id, birimAdi: ozet.birim_adi, dosyaNo: ozet.dosya_no }; }
            else bagli = null;
            dosyaVer(f);
            kayit = { id: ozet.id, surum: surum, ad: ozet.ad };
            if (imz) { imzali = f; imzaliZaman = Date.now(); }
            araclariGuncelle();
            sonBaslik = '';
            bildir('Büro belgesi açıldı: ' + ozet.ad + ' (sürüm ' + surum + (imz ? ', e-imzalı' : '') + ').');
        } catch (x) { bildir('Belge açılamadı: ' + (x && x.message || x)); }
    }
    window.__cuYaziBelgeAc = function(id) { belgeAc({ id: id }); };

    // İş seçici: açık işler (bağlı dosyanınkiler önce), aramayla. sec(is) seçimde.
    function isSecici(sec) {
        const kap = el('div', 'cu-yz-secici');
        const ara = el('input');
        ara.placeholder = 'İş başlığı, dosya no ya da birimle arayın';
        const liste = el('div', 'cu-yz-liste');
        kap.append(ara, liste);
        let zaman = 0, sira = 0;
        async function yukle() {
            const n = ++sira;
            const q = { gorunum: 'tum', durum: 'acik,devam', ara: ara.value.trim(), sinir: 40 };
            if (!q.ara && bagli && bagli.dosyaNo) q.dosya_no = bagli.dosyaNo;
            try {
                let s = await veriAl('liste', q);
                if (!s.length && q.dosya_no) { delete q.dosya_no; s = await veriAl('liste', q); }
                if (n !== sira) return;
                liste.replaceChildren();
                if (!s.length) liste.appendChild(el('div', 'cu-yz-bos', 'Açık iş bulunamadı.'));
                s.forEach(function(g) {
                    const b = el('button', 'cu-yz-oge');
                    b.append(el('b', null, g.baslik), el('span', null, [[g.birim_adi, g.dosya_no].filter(Boolean).join(' '), g.sorumlu_adi, g.son_tarih].filter(Boolean).join(' · ')));
                    b.addEventListener('click', function() { liste.querySelectorAll('.secili').forEach(function(x) { x.classList.remove('secili'); }); b.classList.add('secili'); sec(g); });
                    liste.appendChild(b);
                });
            } catch (x) { liste.replaceChildren(el('div', 'cu-yz-bos', 'İşler alınamadı: ' + (x && x.message || x))); }
        }
        ara.addEventListener('input', function() { clearTimeout(zaman); zaman = setTimeout(yukle, 250); });
        yukle();
        return kap;
    }
    function isePenceresi() {
        if (!kayit) return;
        const k = kutuAc('Belgeyi işe ekle');
        let secilen = null;
        k.kutu.appendChild(isSecici(function(g) { secilen = g; git.disabled = false; }));
        const not = el('div', 'cu-yz-not');
        const alt = el('div', 'cu-yz-alt');
        const vazgec = el('button', null, 'Vazgeç'), git = el('button', 'ana', 'Ekle');
        git.disabled = true;
        vazgec.addEventListener('click', k.kapa);
        git.addEventListener('click', async function() {
            git.disabled = true;
            try { await veriIstek('belge_ise_ekle', { belge: kayit.id, gorev: secilen.id }); k.kapa(); bildir('Belge "' + secilen.baslik + '" işine eklendi.'); }
            catch (x) { not.textContent = 'Eklenemedi: ' + (x && x.message || x); git.disabled = false; }
        });
        alt.append(vazgec, git);
        k.kutu.append(not, alt);
    }
    // Onaya sun (İş Listesi üzerinden): var olan iş ya da yeni "Belge onayı" işi + onaylayıcı.
    function onayPenceresi() {
        if (!kayit) return;
        const k = kutuAc('Onaya sun');
        k.kutu.classList.add('genis');
        const sekmeler = el('div', 'cu-yz-sekmeler');
        const yeniB = el('button', 'secili', 'Yeni onay işi'), varB = el('button', null, 'Var olan işle');
        sekmeler.append(yeniB, varB);
        const yeniKap = el('div');
        const onEt = el('label', 'cu-yz-satir', 'Onaylayacak kişi');
        const onSec = el('select');
        onSec.appendChild(new Option('Yükleniyor…', ''));
        onEt.appendChild(onSec);
        yeniKap.append(el('div', 'cu-yz-satir cu-yz-soluk', '"Belge onayı: ' + kayit.ad + '" işi açılır, sorumlusu siz olursunuz; onaylayıcıya düşer.'), onEt);
        let secilen = null;
        const varKap = isSecici(function(g) { secilen = g; });
        varKap.hidden = true;
        const notEt = el('label', 'cu-yz-satir', 'Not (isteğe bağlı)');
        const notK = el('input');
        notK.maxLength = 500;
        notEt.appendChild(notK);
        const durum = el('div', 'cu-yz-not');
        const alt = el('div', 'cu-yz-alt');
        const vazgec = el('button', null, 'Vazgeç'), git = el('button', 'ana', 'Onaya sun');
        alt.append(vazgec, git);
        k.kutu.append(sekmeler, yeniKap, varKap, notEt, durum, alt);
        let kip = 'yeni';
        yeniB.addEventListener('click', function() { kip = 'yeni'; yeniB.classList.add('secili'); varB.classList.remove('secili'); yeniKap.hidden = false; varKap.hidden = true; });
        varB.addEventListener('click', function() { kip = 'var'; varB.classList.add('secili'); yeniB.classList.remove('secili'); yeniKap.hidden = true; varKap.hidden = false; });
        vazgec.addEventListener('click', k.kapa);
        Promise.all([veriAl('kisiler'), veriAl('ben').catch(function() { return null; })]).then(function(s) {
            const benId = s[1] && s[1].ben && s[1].ben.id;
            onSec.replaceChildren(new Option('Kişi seçin…', ''));
            s[0].filter(function(x) { return x.id !== benId; }).forEach(function(x) { 
                const h = String(x.hesap || '').split('@')[0];
                const ad = (x.ad || '') + (h && String(x.ad || '').toLocaleLowerCase('tr').indexOf(h.toLocaleLowerCase('tr')) < 0 ? ' (' + h + ')' : '');
                onSec.appendChild(new Option(ad + (x.rol === 'yonetici' ? ' · yönetici' : ''), x.id));
            });
        }).catch(function(x) { durum.textContent = 'Kişiler alınamadı: ' + (x && x.message || x); });
        git.addEventListener('click', async function() {
            const govde = { belge: kayit.id, not: notK.value.trim() };
            if (kip === 'yeni') { if (!onSec.value) { durum.textContent = 'Onaylayacak kişiyi seçin.'; return; } govde.onaylayici = Number(onSec.value); }
            else { if (!secilen) { durum.textContent = 'İşi seçin.'; return; } govde.gorev = secilen.id; }
            git.disabled = vazgec.disabled = true;
            durum.textContent = 'Gönderiliyor…';
            try {
                const v = await veriIstek('belge_onaya_sun', govde);
                k.kapa();
                bildir('Belge onaya sunuldu (İş Listesi, iş #' + v.gorev + ').');
            } catch (x) { durum.textContent = 'Onaya sunulamadı: ' + (x && x.message || x); git.disabled = vazgec.disabled = false; }
        });
    }

    // UYAP dosya kartına "Editörde yaz": editör o dosyaya bağlı açılır.
    function kartDugmeleri() {
        if (!window.__uyapKartVerisi || !window.__uyapBaslikDayanagi) return;
        document.querySelectorAll('.dosya-sorgula-popup .dx-popup-title').forEach(function(baslik) {
            if (baslik.querySelector('[data-cu-yazi-git]')) return;
            const dayanak = window.__uyapBaslikDayanagi(baslik);
            const yer = dayanak ? dayanak.parentElement : null;
            if (!yer) return;
            const b = document.createElement('div');
            b.setAttribute('data-cu-yazi-git', '1');
            b.setAttribute('role', 'button');
            b.setAttribute('tabindex', '0');
            b.title = 'Bu dosya için Armonia Yazı\'da belge yaz; imzalanan belge bu dosyaya gönderilebilir';
            b.className = 'dx-widget dx-button dx-button-mode-outlined dx-button-normal dx-button-has-text dx-button-has-icon adalet-button';
            b.style.marginRight = '6px';
            b.innerHTML = '<div class="dx-button-content"><span class="dx-icon fe icon-edit-3"></span> <span>Editörde yaz</span></div>';
            b.addEventListener('click', function(ev) {
                ev.stopPropagation();
                const kart = baslik.closest('.dosya-sorgula-popup');
                const v = kart && window.__uyapKartVerisi(kart);
                if (!v) { bildir('Bu kartın dosya bilgisi okunamadı; kartı kapatıp yeniden açın.'); return; }
                const yeni = !kok || !belgeAdi();
                ac();
                bagla(v);
                if (yeni) yeniBelge();
                bildir('Armonia Yazı ' + dosyaEtiketi(v) + ' dosyasına bağlandı.');
            });
            yer.insertBefore(b, dayanak);
        });
    }
    // Editörde boş belge (kendi "Yeni" düğmesi; çerçeve hazır olunca).
    function yeniBelge(sonra) {
        let n = 0;
        (function dene() {
            const d = cd(), y = d && dugmeBul(d, 'Yeni');
            if (hazir && y) { y.click(); if (sonra) sonra(); return; }
            if (++n < 100) setTimeout(dene, 100);
        })();
    }
    // Dışarıdan yeni belge (İcra dosya paneli, İş Listesi): o = { kart?, buroDosyaId?, birimAdi?, dosyaNo?, gorevId? }
    window.__cuYaziYeni = function(o) {
        o = o || {};
        ac();
        const bag = o.kart && o.kart.dosyaId ? null : (o.buroDosyaId ? { buroDosyaId: o.buroDosyaId, birimAdi: o.birimAdi || '', dosyaNo: o.dosyaNo || '' } : null);
        yeniBelge(function() {
            // "Yeni" tıklaması belgeBirak'ı çağırır; bağ ve iş ondan SONRA kurulur
            if (o.kart && o.kart.dosyaId) bagla(o.kart); else { bagli = bag; araclariGuncelle(); }
            yeniIcin = o.gorevId ? { gorevId: o.gorevId } : null;
            bildir('Yeni belge' + (bagli ? ' (' + dosyaEtiketi(bagli) + ')' : '') + (o.gorevId ? '; ilk kayıtta işe eklenecek' : '') + '.');
        });
    };

    function cerceveHazir() {
        const w = cw(), d = cd();
        if (!w || !d) return;
        hazir = true;
        temaAktar();
        w.addEventListener('keydown', panoTusu, true);
        yakalamaKur(w);
        d.addEventListener('click', imzaDugmesi, true);
        // Kullanıcı editörde başka belge açarsa (sürükle-bırak, "Aç") imzalı dosya artık ona ait değil.
        w.addEventListener('drop', function() { if (!bizimVerme) belgeBirak(); }, true);
        d.addEventListener('change', function(e) { if (e.target && e.target.id === 'dosya') belgeBirak(); }, true);
        // Ctrl+S: büroya kaydet
        w.addEventListener('keydown', function(e) {
            if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && (e.key === 's' || e.key === 'S')) {
                e.preventDefault(); e.stopImmediatePropagation(); buroKaydet();
            }
        }, true);
        // Editör modülü (WebAssembly) yüklenince bekleyen belge verilir.
        let deneme = 0;
        (function bekle() {
            const dd = cd();
            if (dd && dd.querySelector('#arac-cubugu button')) {
                if (bekleyen) { const f = bekleyen; bekleyen = null; dosyaVer(f); }
                return;
            }
            if (++deneme < 100) setTimeout(bekle, 100);
        })();
    }

    // ── Pencere ──
    function kur() {
        stilKur();
        // Editörün ölçek tercihi aynı kökende: hiç seçilmemişse pencereye "sayfa genişliği" ile başlasın.
        try { if (!localStorage.getItem('armonia-yazi-olcek')) localStorage.setItem('armonia-yazi-olcek', 'genislik'); } catch (x) {}
        kok = el('div');
        kok.id = 'cu-yazi';
        kok.setAttribute('role', 'dialog');
        kok.setAttribute('aria-label', 'Armonia Yazı');
        const bas = el('div', 'cu-yz-bas');
        baslikAd = el('div', 'cu-yz-ad');
        const dugmeler = el('span', 'cu-yz-dugmeler');
        const kip = el('button', 'cu-yz-kip', '⇆');
        kip.title = 'Yan yana / serbest pencere';
        kip.addEventListener('click', function() { ayar.kip = ayar.kip === 'yan' ? 'serbest' : 'yan'; ayar.tam = false; ayarYaz(); yerlestir(); dugmeYaz(); });
        const buyut = el('button', 'cu-yz-buyut', '□');
        function dugmeYaz() {
            buyut.textContent = ayar.tam ? '❐' : '□';
            buyut.title = ayar.tam ? 'Yan yana (UYAP solda, editör sağda)' : 'Büyüt (bütün alanı kapla)';
        }
        dugmeYaz();
        buyut.addEventListener('click', function() { ayar.tam = !ayar.tam; ayar.kip = 'yan'; ayarYaz(); yerlestir(); dugmeYaz(); });
        const kucuk = el('button', null, '–');
        kucuk.title = 'Görev çubuğuna küçült (belge açık kalır)';
        kucuk.addEventListener('click', kucult);
        const kapa = el('button', null, '✕');
        kapa.title = 'Kapat';
        kapa.addEventListener('click', function() { kapat(false); });
        dugmeler.append(kip, buyut, kucuk, kapa);
        // Bağlı dosya, Kaydet ▾, Dosyaya gönder
        const bag = el('span', 'cu-yz-bag');
        const bagAd = el('span');
        const bagKaldir = el('button', null, '✕');
        bagKaldir.title = 'Dosya bağlantısını kaldır';
        bagKaldir.addEventListener('click', function() { bagli = null; araclariGuncelle(); });
        bag.append(document.createTextNode('Dosya: '), bagAd, bagKaldir);
        const buroD = el('button', null, 'Kaydet');
        buroD.title = 'Büro veritabanına kaydet (Ctrl+S): her kayıt yeni sürüm, her yerden açılır';
        buroD.addEventListener('click', function() { buroKaydet(); });
        const kaydetD = el('button', 'cu-yz-ok', '▾');
        kaydetD.title = 'Büro belgelerini aç, farklı kaydet, indir (UDF / Word / PDF)';
        kaydetD.addEventListener('click', function() { kaydetMenusu(kaydetD); });
        const gonderD = el('button', 'ana', 'Dosyaya gönder');
        gonderD.addEventListener('click', gonderPenceresi);
        araclar = { bag: bag, bagAd: bagAd, kaydet: kaydetD, gonder: gonderD };
        bas.append(baslikAd, bag, buroD, kaydetD, gonderD, dugmeler);
        araclariGuncelle();
        const tutamac = el('div', 'cu-yz-tutamac');
        cerceve = el('iframe');
        cerceve.title = 'Armonia Yazı';
        cerceve.addEventListener('load', cerceveHazir);
        cerceve.src = ADRES;
        kok.append(tutamac, bas, cerceve);
        // UYAP'ın açılır pencereleri "dışına tıklanınca kapan"masın: penceredeki tıklamalar belgeye ulaşmaz.
        ['mousedown', 'pointerdown', 'touchstart'].forEach(function(t) { kok.addEventListener(t, function(e) { e.stopPropagation(); }); });
        surukleKur(bas, tutamac);
        // Serbest pencerede CSS resize ile değişen boyut saklanır.
        if (window.ResizeObserver) new ResizeObserver(function() {
            if (ayar.kip !== 'yan' && kok.style.display !== 'none' && !mobil()) { ayar.w = kok.offsetWidth; ayar.h = kok.offsetHeight; ayarYaz(); }
        }).observe(kok);
        document.body.appendChild(kok);
        yerlestir();
        izci = setInterval(baslikGuncelle, 800);
        new MutationObserver(temaAktar).observe(document.documentElement, { attributes: true, attributeFilter: ['data-cu-tema', 'data-cu-deri', 'data-theme', 'style', 'class'] });
    }
    function surukleKur(bas, tutamac) {
        let mod = null, x0 = 0, y0 = 0, a = null;
        function bitir() {
            if (!mod) return;
            mod = null;
            kok.classList.remove('surukleniyor');
            ayarYaz();
        }
        function hareket(e) {
            if (!mod) return;
            if (mod === 'tasi') { ayar.x = a.x + e.clientX - x0; ayar.y = a.y + e.clientY - y0; }
            else ayar.gen = a.gen - (e.clientX - x0);
            yerlestir();
        }
        bas.addEventListener('pointerdown', function(e) {
            if (ayar.kip === 'yan' || mobil() || e.target.closest('button')) return;
            mod = 'tasi'; x0 = e.clientX; y0 = e.clientY; a = { x: kok.offsetLeft, y: kok.offsetTop };
            kok.classList.add('surukleniyor');
        });
        tutamac.addEventListener('pointerdown', function(e) {
            mod = 'gen'; x0 = e.clientX; a = { gen: kok.offsetWidth };
            kok.classList.add('surukleniyor');
            e.preventDefault();
        });
        window.addEventListener('pointermove', hareket);
        window.addEventListener('pointerup', bitir);
        window.addEventListener('pointercancel', bitir);
    }
    let sonBaslik = '';
    function baslikGuncelle() {
        // "Düzenlemeyi aç" (imza şeridi kalktı): imzalı dosya artık editördekiyle aynı değil
        if (imzali && Date.now() - imzaliZaman > 6000 && !imzaSeridi()) imzaliBirak();
        if (!kok || !baslikAd || baslikAd.dataset.soru) return;
        const ad = belgeAdi(), dg = degisti();
        const durumMetni = kayit ? 'Büroda · sürüm ' + kayit.surum : (ad ? 'büroya kaydedilmedi' : '');
        const m = (ad || '') + (dg ? ' •' : '') + '|' + durumMetni;
        if (m === sonBaslik && baslikAd.firstChild) return;
        sonBaslik = m;
        const parcalar = [el('b', null, 'Armonia Yazı'), el('span', 'cu-yz-adm', ad ? ad + (dg ? ' •' : '') : '')];
        if (durumMetni) parcalar.push(el('span', 'cu-yz-durum' + (kayit ? ' kayitli' : ''), durumMetni));
        baslikAd.replaceChildren.apply(baslikAd, parcalar);
        baslikAd.title = ad ? ad + (dg ? ' (kaydedilmemiş değişiklik var)' : '') + (kayit ? ' — büro veritabanında, sürüm ' + kayit.surum : ' — büroya kaydedilmedi (Ctrl+S)') : 'Armonia Yazı';
        gorevGuncelle();
    }

    function ac() {
        if (!kok) kur();
        durum = 'acik';
        kok.style.display = '';
        yerlestir();
        gorevGuncelle();
        try { cerceve.focus(); } catch (x) {}
    }
    function kucult() {
        if (!kok) return;
        durum = 'kucuk';
        kok.style.display = 'none';
        gorevGuncelle();
    }
    // Kaydedilmemiş değişiklik varsa editörün üstünde onay penceresi açılır (tarayıcı uyarı penceresi değil). Eskiden
    // başlık çubuğunda silik bir yazıydı; kullanıcı fark etmeyip "Kapat çalışmıyor" sanıyordu (26.11.2026).
    let kapatSorusu = null;
    function kapat(zorla) {
        if (!kok) return;
        if (!zorla && degisti()) {
            if (kapatSorusu) { kapatSorusu.odak(); return; }
            if (durum !== 'acik') ac();
            const k = kutuAc('Kaydedilmemiş değişiklikler var');
            const ad = belgeAdi();
            k.kutu.appendChild(el('div', 'cu-yz-satir', (ad ? '"' + ad + '" belgesinde' : 'Belgede') +
                ' büroya kaydedilmemiş değişiklikler var. Kaydetmeden kapatırsanız bu değişiklikler kaybolur.'));
            const not = el('div', 'cu-yz-not', '');
            k.kutu.appendChild(not);
            const alt = el('div', 'cu-yz-alt');
            const vazgec = el('button', null, 'Vazgeç');
            const kaydetmeden = el('button', null, 'Kaydetmeden kapat');
            kaydetmeden.style.color = '#b84444';
            const kaydet = el('button', 'ana', kayit ? 'Büroya kaydet ve kapat' : 'Büroya kaydet…');
            const bitir = function() { kapatSorusu = null; k.kapa(); };
            vazgec.addEventListener('click', bitir);
            kaydetmeden.addEventListener('click', function() { bitir(); kapat(true); });
            kaydet.addEventListener('click', async function() {
                if (!kayit) { bitir(); kaydetPenceresi(false); return; }   // ilk kayıt: ad / dosya sorulur
                kaydet.disabled = kaydetmeden.disabled = vazgec.disabled = true;
                not.textContent = 'Kaydediliyor…';
                try {
                    const v = await kaydedilecek();
                    await buroyaYaz(v.f, { imzali: v.imzali });
                    bitir();
                    kapat(true);
                } catch (x) {
                    kaydet.disabled = kaydetmeden.disabled = vazgec.disabled = false;
                    not.textContent = x && x.kod === 'CAKISMA' ? x.message : 'Kaydedilemedi: ' + (x && x.message || x);
                }
            });
            alt.append(vazgec, kaydetmeden, kaydet);
            k.kutu.appendChild(alt);
            // Dış tık / Esc kutuAc'ta pencereyi kapatır: soru da temizlensin.
            const kat = k.kutu.parentElement;
            kat.addEventListener('pointerdown', function(e) { if (e.target === kat) kapatSorusu = null; });
            kat.addEventListener('keydown', function(e) { if (e.key === 'Escape') kapatSorusu = null; });
            kapatSorusu = { odak: function() { try { vazgec.focus(); } catch (x) {} } };
            setTimeout(kapatSorusu.odak, 0);
            return;
        }
        kapatSorusu = null;
        clearInterval(izci);
        kok.remove();
        kok = cerceve = baslikAd = araclar = null;
        imzali = null; bagli = null;
        hazir = false; bekleyen = null; sonBaslik = '';
        durum = 'kapali';
        gorevGuncelle();
    }
    function gorevGuncelle() {
        if (!window.CU || !window.CU.gorev) return;
        if (durum === 'kapali') { window.CU.gorev.ayarla([], 'yazi'); return; }
        const ad = belgeAdi();
        window.CU.gorev.ayarla([{
            kimlik: 'yazi', ad: 'Armonia Yazı' + (ad ? ': ' + ad : ''), ikon: '✎', on: durum === 'acik', kucuk: durum !== 'acik',
            rozet: degisti() ? '•' : '',
            tikla: function() { if (durum === 'acik') kucult(); else ac(); },
            kapat: function() { if (durum !== 'acik') ac(); kapat(false); },
            menu: function() { return [[[durum === 'acik' ? 'Küçült' : 'Öne getir', function() { if (durum === 'acik') kucult(); else ac(); }]]]; },
        }], 'yazi');
    }

    // Sol menü girişi (İcra Takip Süreçleri'nin altında).
    function menuGirisi() {
        const yan = document.getElementById('av-sidebar-wrapper');
        if (!yan || document.getElementById('cu-menu-yazi')) return;
        const ust = yan.querySelector('.sidebar-top-part') || yan.firstElementChild || yan;
        const ul = el('ul', 'menu-list');
        ul.id = 'cu-menu-yazi';
        const li = el('li', 'SidebarLink'), a = el('a');
        a.href = '#';
        a.title = 'Armonia Yazı: UDF belgesi aç, yaz, kaydet (UYAP belgesinin yanında açılır)';
        a.appendChild(el('span', 'SidebarLabel', 'Armonia Yazı'));
        a.addEventListener('click', function(ev) { ev.preventDefault(); ac(); });
        li.appendChild(a);
        ul.appendChild(li);
        const icra = document.getElementById('cu-menu-icra');
        if (icra && icra.parentNode === ust) ust.insertBefore(ul, icra.nextSibling); else ust.insertBefore(ul, ust.firstChild);
    }

    // UYAP dosya kartının sağda bırakacağı pay (kartlar.js kartiHizala): yan yana açıkken editörün genişliği.
    window.__cuYaziSagPay = function() {
        return kok && durum === 'acik' && ayar.kip === 'yan' && !ayar.tam && !mobil() && kok.style.display !== 'none' ? kok.offsetWidth : 0;
    };
    window.__cuYaziAc = ac;
    // v: belgenin geldiği UYAP dosyası (kart verisi; paylas.js "Yazı'da aç") → editör o dosyaya bağlanır
    window.__cuYaziDosyaAc = function(f, v) { ac(); belgeBirak(); if (v) bagla(v); dosyaVer(f); };
    window.addEventListener('resize', yerlestir);
    if (window.__cuKipDegisince) window.__cuKipDegisince(yerlestir);
    setInterval(function() { menuGirisi(); kartDugmeleri(); }, 1000);
})();
