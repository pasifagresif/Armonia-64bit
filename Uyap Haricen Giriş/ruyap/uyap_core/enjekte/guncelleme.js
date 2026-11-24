// ═══════════════════════════════════════════════════════════════════════════════════════
// ÇOKLU UYAP — GÜNCELLEME UYARISI (2026-11-23)
// ═══════════════════════════════════════════════════════════════════════════════════════
// Program (src/guncelleme.rs) cokluuyap.com'daki imzalı sürüm dosyasını denetler; bu betik yalnız sonucu gösterir:
// yeni sürüm varsa görev çubuğunda "Güncelleme" düğmesi, tıklanınca sürüm notları ve "Şimdi kur" / "Sonra".
// Kurulum her seferinde kullanıcı onayıyla başlar (kullanıcı kararı 23.11.2026). Sürüm dosyasında "zorunlu"
// işaretliyse pencere kendiliğinden açılır ve "Sonra" düğmesi olmaz. Uçlar: GET /__cu__/guncelleme (durum),
// POST /__cu__/guncelleme/kur (onaylı kurulum; yalnız bu bilgisayardan).
(function() {
    'use strict';
    const anaCerceve = window.__uyapAnaCerceve ? window.__uyapAnaCerceve() : window.top === window;
    if (!anaCerceve || window.__cuGuncelleme) return;
    window.__cuGuncelleme = true;

    const UC = '/__cu__/guncelleme';
    let durum = null, pencere = null, gizli = false, zorunluGosterildi = false, hizliZaman = null, istekSuruyor = false;

    function el(tag, sinif, metin) {
        const e = document.createElement(tag);
        if (sinif) e.className = sinif;
        if (metin != null) e.textContent = metin;
        return e;
    }
    function mb(n) { return (n / 1048576).toLocaleString('tr-TR', { maximumFractionDigits: 1 }) + ' MB'; }

    function stilKur() {
        if (document.getElementById('cu-gnc-stil')) return;
        const s = el('style');
        s.id = 'cu-gnc-stil';
        s.textContent = [
            '#cu-gnc-ortu{position:fixed;inset:0;z-index:2147483600;background:rgba(0,0,0,.4);display:flex;align-items:center;justify-content:center;padding:16px}',
            '#cu-gnc{--g-yuzey:var(--cu-t-yuzey,#fff);--g-yazi:var(--cu-t-yazi,#212529);--g-kenar:var(--cu-t-kenar,#dee2e6);',
            '--g-soluk:color-mix(in srgb,var(--g-yazi) 80%,var(--g-yuzey));width:min(460px,100%);max-height:calc(100vh - 32px);overflow:auto;',
            'background:var(--g-yuzey);color:var(--g-yazi);border:1px solid var(--g-kenar);border-radius:14px;box-shadow:0 18px 50px rgba(0,0,0,.3);',
            'padding:18px 20px;font:14px/1.5 "Segoe UI",system-ui,sans-serif}',
            'html[data-cu-tema="koyu"] #cu-gnc{--g-yuzey:#2a2f40;--g-yazi:#dedede;--g-kenar:#3a4055}',
            '#cu-gnc h2{font-size:17px;font-weight:600;margin:0 0 4px}',
            '#cu-gnc .alt{color:var(--g-soluk);font-size:12.5px}',
            '#cu-gnc .notlar{white-space:pre-wrap;margin:12px 0;padding:10px 12px;border:1px solid var(--g-kenar);border-radius:10px;max-height:40vh;overflow:auto}',
            '#cu-gnc .uyari{margin:10px 0 0;font-size:12.5px;color:var(--g-soluk)}',
            '#cu-gnc .hata{margin:10px 0 0;padding:8px 11px;border-radius:9px;background:color-mix(in srgb,#b84444 12%,var(--g-yuzey));color:color-mix(in srgb,#b84444 80%,var(--g-yazi))}',
            '#cu-gnc .ilerleme{height:6px;border-radius:3px;background:var(--g-kenar);margin:12px 0 4px;overflow:hidden}',
            '#cu-gnc .ilerleme i{display:block;height:100%;background:var(--cu-vurgu,#2d4596);width:0;transition:width .3s}',
            '#cu-gnc .dugmeler{display:flex;gap:8px;justify-content:flex-end;margin-top:16px;flex-wrap:wrap}',
            '#cu-gnc button{font:inherit;border:1px solid var(--g-kenar);background:var(--g-yuzey);color:var(--g-yazi);border-radius:9px;padding:7px 14px;cursor:pointer}',
            '#cu-gnc button.ana{background:var(--cu-vurgu,#2d4596);border-color:var(--cu-vurgu,#2d4596);color:var(--cu-vurgu-uzeri,#fff);font-weight:600}',
            '#cu-gnc button:disabled{opacity:.55;cursor:default}',
        ].join('\n');
        document.head.appendChild(s);
    }

    async function yokla(yenile) {
        if (istekSuruyor) return;
        istekSuruyor = true;
        try {
            const r = await fetch(UC + (yenile ? '?yenile=1' : ''), { cache: 'no-store', credentials: 'same-origin' });
            if (r.ok) { durum = await r.json(); guncelle(); }
        } catch (x) { /* sessiz: bir sonraki yoklamada */ }
        istekSuruyor = false;
    }

    function suruyor() { return !!durum && (durum.asama === 'indiriliyor' || durum.asama === 'kuruluyor'); }

    function guncelle() {
        const y = durum && durum.yeni;
        if (window.CU && window.CU.gorev) {
            const goster = !!y && (!gizli || y.zorunlu || suruyor());
            window.CU.gorev.ayarla(goster ? [{
                kimlik: 'guncelleme', ad: 'Güncelleme: ' + y.surum, ikon: '⬆', on: !!pencere, kucuk: !pencere,
                rozet: suruyor() ? '…' : '!', tikla: function() { if (pencere) kapat(); else ac(); },
                kapat: function() { if (y.zorunlu) return; gizli = true; kapat(); guncelle(); },
                menu: function() { return [[['Ayrıntılar', ac]]]; },
            }] : [], 'guncelleme');
        }
        if (y && y.zorunlu && !zorunluGosterildi && !pencere) { zorunluGosterildi = true; ac(); }
        if (pencere) ciz();
        // İndirme / kurulum sürerken sık yoklanır.
        clearTimeout(hizliZaman);
        if (suruyor()) hizliZaman = setTimeout(function() { yokla(false); }, 1000);
    }

    function ac() {
        if (!durum || !durum.yeni) return;
        stilKur();
        if (!pencere) {
            pencere = el('div');
            pencere.id = 'cu-gnc-ortu';
            pencere.addEventListener('mousedown', function(e) { if (e.target === pencere && !durum.yeni.zorunlu && !suruyor()) kapat(); });
            pencere.addEventListener('keydown', function(e) { if (e.key === 'Escape' && !durum.yeni.zorunlu && !suruyor()) kapat(); });
            document.body.appendChild(pencere);
        }
        ciz();
        guncelle();
    }
    function kapat() {
        if (pencere) pencere.remove();
        pencere = null;
        guncelle();
    }

    function ciz() {
        const y = durum.yeni;
        const kutu = el('div');
        kutu.id = 'cu-gnc';
        kutu.setAttribute('role', 'dialog');
        kutu.setAttribute('aria-modal', 'true');
        kutu.setAttribute('aria-label', 'Çoklu UYAP güncellemesi');
        if (!y) { kapat(); return; }
        kutu.appendChild(el('h2', null, y.zorunlu ? 'Zorunlu güncelleme: ' + y.surum : 'Yeni sürüm: ' + y.surum));
        kutu.appendChild(el('div', 'alt', 'Kurulu sürüm ' + durum.kurulu + (y.tarih ? ' · yayın ' + String(y.tarih).slice(0, 10) : '') + ' · ' + mb(y.boyut)));
        if (y.notlar) kutu.appendChild(el('div', 'notlar', y.notlar));
        if (durum.asama === 'indiriliyor') {
            const c = el('div', 'ilerleme'), i = el('i');
            i.style.width = Math.min(100, Math.round((durum.inen || 0) * 100 / (y.boyut || 1))) + '%';
            c.appendChild(i);
            kutu.append(c, el('div', 'alt', 'İndiriliyor: ' + mb(durum.inen || 0) + ' / ' + mb(y.boyut)));
        } else if (durum.asama === 'kuruluyor') {
            kutu.appendChild(el('div', 'uyari', durum.mesaj || 'Kuruluyor… Program kapanıp yeni sürümle açılacak.'));
        } else if (durum.asama === 'indirildi') {
            kutu.appendChild(el('div', 'uyari', durum.mesaj || 'Paket indirildi ve doğrulandı.'));
        } else {
            kutu.appendChild(el('div', 'uyari', 'Paketin imzası ve bütünlüğü kurulmadan önce denetlenir. Kurulum sırasında program kapanıp yeniden açılır; e-imza PIN\'inizi yeniden girmeniz gerekir.'));
        }
        if (durum.hata) kutu.appendChild(el('div', 'hata', durum.hata));
        const d = el('div', 'dugmeler');
        if (!y.zorunlu && !suruyor()) {
            const sonra = el('button', null, 'Sonra');
            sonra.type = 'button';
            sonra.addEventListener('click', kapat);
            d.appendChild(sonra);
        }
        if (durum.asama !== 'indirildi') {
            const kur = el('button', 'ana', suruyor() ? (durum.asama === 'kuruluyor' ? 'Kuruluyor…' : 'İndiriliyor…') : 'Şimdi kur');
            kur.type = 'button';
            kur.disabled = suruyor();
            kur.addEventListener('click', kurBaslat);
            d.appendChild(kur);
        }
        kutu.appendChild(d);
        pencere.replaceChildren(kutu);
        const odak = kutu.querySelector('button.ana:not(:disabled)') || kutu.querySelector('button');
        if (odak && !kutu.contains(document.activeElement)) odak.focus();
    }

    async function kurBaslat() {
        try {
            const r = await fetch(UC + '/kur', { method: 'POST', cache: 'no-store', credentials: 'same-origin' });
            const j = await r.json().catch(function() { return {}; });
            if (r.ok) durum = j;
            else durum = Object.assign({}, durum, { hata: j.hata || 'Kurulum başlatılamadı.' });
        } catch (x) {
            durum = Object.assign({}, durum, { hata: 'Programa ulaşılamadı.' });
        }
        guncelle();
    }

    // İlk yoklama sayfa açılınca (program tarafında en fazla dakikada bir denetim), sonra 30 dakikada bir.
    setTimeout(function() { yokla(true); }, 4000);
    setInterval(function() { yokla(false); }, 30 * 60 * 1000);
})();
