// ═══════════════════════════════════════════════════════════════════════════════════════
// ÇOKLU UYAP — BELGE GÖRÜNTÜLEYİCİ: İNDİR / PAYLAŞ DÜĞMELERİ (2026-11-17, 10. sürüm)
// ═══════════════════════════════════════════════════════════════════════════════════════
// Belge görüntüleyicilerin araç çubuğuna kendi tasarımımızla doğrudan düğmeler (açılır pencere / ara tık YOK;
// kullanıcı isteği 17.11.2026: "kullanıcı ayrı tıklama yapmadan işini çözsün"). UYAP'ın indirme simgeleri gizlenir.
//   · UDF okuyucusu (.ude-document-viewer-toolbar):   [UDF İndir] [UDF Paylaş]  [PDF İndir] [PDF Paylaş]
//       UYAP'ın title="İndir" (<a download>.udf) ve title="PDF Olarak İndir" (showSaveFilePicker .pdf) düğmelerine
//       program basar, çıktıyı yakalar. Her tık belgeyi UYAP'tan yeniden alır (aynı adlı belgeler karışmasın).
//   · PDF görüntüleyicisi (.documentViewerHeader + react-pdf):   yalnız [PDF İndir] [PDF Paylaş]
//       PDF bileşende hazır (file="data:application/pdf;base64,…"), UYAP'a istek gitmez.
//   · Öbür görüntüleyici (aria-label="show_download"): aslı + PDF.
// Başka hiçbir ekrana / indirmeye (Talep Evrakı Oluştur, Evrak İndir …) dokunulmaz.
// Yapı canlı sayfada doğrulandı (17.11.2026) — değiştirmeden önce yine canlı DOM'a bak.
//
//   İndir:   "farklı kaydet" penceresi (izin dolmuşsa tarayıcı indirmesi).
//   Paylaş:  Bu bilgisayarda TEK TIK, PENCERESİZ (17.11.2026): program belgeyi DOSYA OLARAK panoya koyar
//            (/__uyap_yerel__/paylas?kip=pano); kullanıcı WhatsApp/Outlook/klasörde Ctrl+V ile yapıştırır.
//            Uzak bilgisayarda (cokluuyap.com, 19.11.2026): o bilgisayarın Çoklu UYAP'ı (yerel yardımcı 127.0.0.1:8801)
//            panoya koyar; program yoksa indirilir. Telefon / tablet: cihazın paylaş ekranı (UDF → PDF).
//            Tarayıcının ara paylaşım penceresi masaüstünde hiç açılmaz.
//
// UYAP'ın e-imza "İmzalar" rozeti ([aria-label="sign_info"]) gösterilmez; kendiliğinden açılan imza
// doğrulama penceresi kapatılır (kullanıcı isteği 2026-11-16).
(function() {
    'use strict';
    if (window.__cuPaylas) return;
    window.__cuPaylas = true;

    const YOL_PDF = 'M20 2H8', YOL_INDIR = 'M5 20h14';
    const PAYLAS_SVG = 'M18 16.08c-.76 0-1.44.3-1.96.77L8.91 12.7c.05-.23.09-.46.09-.7s-.04-.47-.09-.7l7.05-4.11c.54.5 1.25.81 2.04.81 1.66 0 3-1.34 3-3s-1.34-3-3-3-3 1.34-3 3c0 .24.04.47.09.7L8.04 9.81C7.5 9.31 6.79 9 6 9c-1.66 0-3 1.34-3 3s1.34 3 3 3c.79 0 1.5-.31 2.04-.81l7.12 4.16c-.05.21-.08.43-.08.65 0 1.61 1.31 2.92 2.92 2.92s2.92-1.31 2.92-2.92-1.31-2.92-2.92-2.92';
    const INDIR_SVG = 'M5 20h14v-2H5zM19 9h-4V3H9v6H5l7 7z';
    const YAZI_SVG = 'M3 17.25V21h3.75L17.81 9.94l-3.75-3.75zM20.71 7.04a1 1 0 0 0 0-1.41l-2.34-2.34a1 1 0 0 0-1.41 0l-1.83 1.83 3.75 3.75z';
    const TURLER = { pdf: 'application/pdf', udf: 'application/octet-stream', png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg',
                     tif: 'image/tiff', tiff: 'image/tiff', txt: 'text/plain', html: 'text/html', htm: 'text/html', rtf: 'application/rtf' };
    const MIME_UZANTI = { 'text/udf': 'udf', 'application/pdf': 'pdf', 'image/tiff': 'tif', 'image/png': 'png', 'image/jpeg': 'jpg',
                          'text/html': 'html', 'text/plain': 'txt', 'application/rtf': 'rtf' };
    const asilTikla = HTMLAnchorElement.prototype.click;
    const asilSecici = window.showSaveFilePicker;
    const asilOlustur = URL.createObjectURL;

    function bildir(m) { try { if (window.__uyapBildirim) window.__uyapBildirim(m); } catch (x) {} }
    function uzanti(ad) { const m = /\.([a-z0-9]{1,6})$/i.exec(ad || ''); return m ? m[1].toLowerCase() : ''; }
    function tur(ad, varsayilan) { return TURLER[uzanti(ad)] || varsayilan || 'application/octet-stream'; }
    function etiketi(u) { return u === 'tif' ? 'TIFF' : u === 'htm' ? 'HTML' : u === 'jpeg' ? 'JPG' : (u || 'UDF').toUpperCase(); }
    function temizAd(s) { return String(s || 'belge').replace(/[\\/:*?"<>|]+/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 120) || 'belge'; }
    function lif(el) { const k = el && Object.keys(el).find(function(x) { return x.indexOf('__reactFiber') === 0; }); return k ? el[k] : null; }
    function propBul(el, sinama, derinlik) {
        let f = lif(el);
        for (let i = 0; f && i < (derinlik || 20); i++, f = f.return) {
            const p = f.memoizedProps;
            if (p && typeof p === 'object' && sinama(p)) return p;
        }
        return null;
    }
    function dugmeTuru(b) {
        const p = b && b.querySelector && b.querySelector('path');
        const d = p ? p.getAttribute('d') || '' : '';
        return d.indexOf(YOL_PDF) === 0 ? 'pdf' : d.indexOf(YOL_INDIR) === 0 ? 'orijinal' : '';
    }

    // ── UYAP düğmesinin çıktısını yakalama (yalnız biz istediğimizde; yoksa UYAP'ın işi aynen) ──
    const bloblar = new Map();
    URL.createObjectURL = function(o) {
        const u = asilOlustur.apply(URL, arguments);
        if (o instanceof Blob) {
            bloblar.set(u, o);
            if (bloblar.size > 20) bloblar.delete(bloblar.keys().next().value);
        }
        return u;
    };
    let yakalayici = null;
    function teslim(f, kaydet) {
        if (!yakalayici) return false;
        const y = yakalayici; yakalayici = null; y({ f: f, kaydet: kaydet });
        return true;
    }
    function yakala(dugme) {
        return new Promise(function(coz, red) {
            if (yakalayici) { red(new Error('Bir belge zaten alınıyor, lütfen bekleyin.')); return; }
            const zaman = setTimeout(function() { if (yakalayici === bitir) yakalayici = null; red(new Error('UYAP belgeyi vermedi (zaman aşımı).')); }, 45000);
            function bitir(s) { clearTimeout(zaman); coz(s); }
            yakalayici = bitir;
            try { dugme.click(); } catch (x) { yakalayici = null; clearTimeout(zaman); red(x); }
        });
    }
    function parcaBlob(v) {
        if (v instanceof Blob) return v;
        if (v && typeof v === 'object' && v.type === 'write' && 'data' in v) return parcaBlob(v.data);
        return new Blob([v]);
    }
    function toastSustur() {
        const bas = Date.now();
        const t = setInterval(function() {
            document.querySelectorAll('.dx-toast-wrapper, .dx-toast-content').forEach(function(x) {
                if (/Dosya başarıyla kaydedildi/.test(x.textContent || '')) x.style.display = 'none';
            });
            if (Date.now() - bas > 4000) clearInterval(t);
        }, 100);
    }
    if (asilSecici) {
        window.showSaveFilePicker = async function(secenek) {
            if (!yakalayici) return asilSecici.apply(window, arguments);
            const ad = (secenek && secenek.suggestedName) || 'belge';
            const parca = [];
            return { kind: 'file', name: ad, createWritable: async function() {
                return { write: async function(v) { parca.push(parcaBlob(v)); },
                         seek: async function() {}, truncate: async function() {}, abort: async function() {},
                         close: async function() {
                             toastSustur();
                             const f = new File(parca, ad, { type: tur(ad) });
                             if (!teslim(f, null)) seciciyleKaydet(f);
                         } };
            } };
        };
    }
    HTMLAnchorElement.prototype.click = function() {
        const href = this.href || '';
        if (yakalayici && this.hasAttribute('download') && !this.getAttribute('data-uyap-internal') && /^(blob|data):/i.test(href)) {
            const ad = this.download || 'belge';
            const b = bloblar.get(href);   // UYAP adresi tıkın hemen ardından siliyor: blob'u önceden tutuyoruz
            const ver = function(x) { teslim(new File([x], ad, { type: tur(ad, x.type) }), null); };
            if (b) ver(b);
            else { const a = this; fetch(href).then(function(r) { return r.blob(); }).then(ver).catch(function() { asilTikla.call(a); }); }
            return;
        }
        return asilTikla.apply(this, arguments);
    };

    // ── İndir / Paylaş ──
    function indir(dosya) {
        const a = document.createElement('a');
        a.href = asilOlustur.call(URL, dosya);
        a.download = dosya.name;
        a.setAttribute('data-uyap-internal', '1');
        document.body.appendChild(a);
        asilTikla.call(a);
        setTimeout(function() { URL.revokeObjectURL(a.href); a.remove(); }, 60000);
    }
    async function seciciyleKaydet(f) {
        if (!asilSecici) { indir(f); return; }
        try {
            const u = uzanti(f.name);
            const h = await asilSecici.call(window, { suggestedName: f.name,
                types: u ? [{ description: 'Dosya', accept: { [tur(f.name)]: ['.' + u] } }] : [] });
            const w = await h.createWritable(); await w.write(f); await w.close();
            bildir('Kaydedildi: ' + (h.name || f.name));
        } catch (x) {
            if (x && x.name === 'AbortError') return;
            indir(f);   // tık izni dolduysa (belge geç geldi) tarayıcı indirmesi
        }
    }
    function kartBilgisi(el) {
        try {
            const kart = el && el.closest && el.closest('.dosya-sorgula-popup');
            const d = kart && window.__uyapKartVerisi ? window.__uyapKartVerisi(kart) : null;
            return d ? [d.birimAdi, d.dosyaNo].filter(Boolean).join(' ').replace(/\s+/g, ' ').trim() : '';
        } catch (x) { return ''; }
    }
    // UTF-8 → Base64 (Türkçe harfler adres satırında bozuluyordu; program başlıktan okur)
    function b64(s) {
        const b = new TextEncoder().encode(String(s || ''));
        let t = '';
        for (let i = 0; i < b.length; i++) t += String.fromCharCode(b[i]);
        return btoa(t);
    }
    function buBilgisayar() { return /^(127\.0\.0\.1|localhost|\[::1\]|::1)$/i.test(location.hostname); }
    function tarayiciPaylasabilir(dosya) {
        if (uzanti(dosya.name) === 'udf') return false;   // Chrome UDF'yi paylaştırmaz
        try { return !!(navigator.canShare && navigator.share && navigator.canShare({ files: [dosya] })); } catch (x) { return false; }
    }
    // Bu bilgisayarda HER ZAMAN programın açtığı Windows paylaşım ekranı; Chrome'un paylaşım penceresine geçilmez
    // (kullanıcı 17.11.2026: "direkt paylaşım ekranı çıksın, saçma sapan pencereler açılmasın").
    async function programlaPaylas(dosya, bilgi) {
        let r, m = '';
        try {
            r = await fetch('/__uyap_yerel__/paylas?kip=pano&ad=' + encodeURIComponent(dosya.name) + '&aciklama=' + encodeURIComponent(bilgi),
                            { method: 'POST', body: dosya, credentials: 'include', cache: 'no-store',
                              headers: { 'X-Cu-Ad': b64(dosya.name), 'X-Cu-Aciklama': b64(bilgi || ' ') } });
            m = (await r.text()).trim();
            if (r.status === 404 || r.status === 405 || r.status === 501) m = 'Program bu sürümde paylaşımı desteklemiyor; programı yeniden başlatın.';
            else if (/^</.test(m) || m.length > 240) m = 'Belge panoya kopyalanamadı (' + r.status + ').';
        } catch (x) { m = 'Programa ulaşılamadı.'; }
        if (r && r.ok && m === 'pano') return;
        if (r && r.ok) m = 'Program eski sürüm; programı yeniden başlatın.';
        throw new Error((m || 'Belge panoya kopyalanamadı.') + ' Bir kez daha deneyin.');
    }
    // Bu bilgisayarda HER TÜR (PDF dahil) programdan: Chrome'un paylaşım yolu Türkçe harfli dosya adını bozuyor
    // (canlı 17.11.2026: "Taşınmaz…" → "TaÅÄ±nmaz…"); programın yazdığı adlar doğru (diskte doğrulandı).
    // Programın yardımcı penceresi saydam, ekranda yalnız Windows paylaşım ekranı görünür.
    // Program açamazsa Chrome'a düşülür — ad bozulmasın diye Türkçe harfler sadeleştirilir.
    function asciiAd(f) {
        const tr = { 'ç': 'c', 'Ç': 'C', 'ğ': 'g', 'Ğ': 'G', 'ı': 'i', 'İ': 'I', 'ö': 'o', 'Ö': 'O', 'ş': 's', 'Ş': 'S', 'ü': 'u', 'Ü': 'U' };
        const ad = f.name.replace(/[çÇğĞıİöÖşŞüÜ]/g, function(c) { return tr[c]; }).normalize('NFKD').replace(/[^ -~]/g, '');
        return ad === f.name ? f : new File([f], ad || 'belge', { type: f.type });
    }
    async function tarayiciylaPaylas(dosya, bilgi) {
        try {
            await navigator.share({ files: [dosya], title: dosya.name });
        } catch (x) {
            if (x && x.name === 'AbortError') return;   // kullanıcı vazgeçti
            if (x && x.name === 'NotAllowedError') throw new Error('Belge hazır; paylaşmak için "Paylaş"a bir kez daha basın.');
            throw new Error('Paylaşım ekranı açılamadı (' + ((x && x.name) || 'hata') + ').');
        }
    }
    // ── Farenin yanında "Kopyalandı" kutusu (kullanıcı isteği 17.11.2026: aşağıdaki bildirim gözden kaçıyor,
    //    çabuk kapanıyor). Kapatma düğmesi yok: ~2,5 sn sonra kendiliğinden söner (kullanıcı isteği); üstüne gelinirse bekler,
    //    tıklanınca ya da bir tuşa basılınca hemen kapanır.
    let sonFare = null;
    document.addEventListener('pointerdown', function(e) { sonFare = { x: e.clientX, y: e.clientY }; }, true);
    function fareUyarisi(ad) {
        const eski = document.getElementById('cu-pano-uyari');
        if (eski) eski.remove();
        const k = document.createElement('div');
        k.id = 'cu-pano-uyari';
        k.setAttribute('role', 'status');
        const ic = document.createElement('div'); ic.className = 'cu-pu-ic';
        ic.innerHTML = '<div class="cu-pu-simge">✓</div><div class="cu-pu-metin"><b>Kopyalandı</b><span class="cu-pu-ad"></span>'
            + '<span>WhatsApp, e-posta ya da klasörde <kbd>Ctrl</kbd> + <kbd>V</kbd> ile yapıştırın.</span></div>';
        ic.querySelector('.cu-pu-ad').textContent = ad;
        const cizgi = document.createElement('div'); cizgi.className = 'cu-pu-sure';
        k.appendChild(ic); k.appendChild(cizgi);
        document.body.appendChild(k);
        // Konum: farenin sağ altı; ekrana sığmazsa sola / yukarı kayar.
        const f = sonFare || { x: innerWidth / 2, y: innerHeight / 3 };
        const g = k.offsetWidth, y = k.offsetHeight, bosluk = 14;
        let x = f.x + bosluk, t = f.y + bosluk;
        if (x + g > innerWidth - 8) x = Math.max(8, f.x - g - bosluk);
        if (t + y > innerHeight - 8) t = Math.max(8, f.y - y - bosluk);
        k.style.left = x + 'px'; k.style.top = t + 'px';
        requestAnimationFrame(function() { k.classList.add('cu-pu-acik'); });
        const SURE = 2500;
        let zam = null, bitti = false;
        function kapat() {
            if (bitti) return; bitti = true;
            clearTimeout(zam);
            document.removeEventListener('keydown', kapat, true);
            k.classList.remove('cu-pu-acik');
            setTimeout(function() { k.remove(); }, 250);
        }
        // Süre fare üstüne gelince DURMAZ (kullanıcı isteği 17.11.2026: pencere sabit kalıyordu).
        function baslat() { zam = setTimeout(kapat, SURE); cizgi.style.transition = 'transform ' + SURE + 'ms linear'; cizgi.style.transform = 'scaleX(0)'; }
        k.addEventListener('click', kapat);
        setTimeout(function() { document.addEventListener('keydown', kapat, true); }, 300);
        requestAnimationFrame(function() { requestAnimationFrame(baslat); });
    }
    // ── Uzak cihaz (cokluuyap.com) — kullanıcı kararı 19.11.2026 ──
    //   · Bilgisayar: o bilgisayarda Çoklu UYAP açıksa belge ONA verilir (yerel yardımcı 127.0.0.1:8801) ve panoya konur
    //     → ofisteki gibi "Kopyalandı · Ctrl+V". Program yoksa belge indirilir. Tarayıcının ara paylaşım penceresi AÇILMAZ.
    //   · Telefon / tablet: doğrudan cihazın paylaş ekranı. UDF'yi o ekran almadığı için PDF'i paylaşılır.
    const YARDIMCI = 'http://127.0.0.1:8801';
    function mobilMi() {
        try { if (navigator.userAgentData && typeof navigator.userAgentData.mobile === 'boolean' && navigator.userAgentData.mobile) return true; } catch (x) {}
        const ua = navigator.userAgent || '';
        // iPadOS kendini Mac olarak tanıtır; dokunmatik Mac yoktur.
        return /Android|iPhone|iPad|iPod|Mobile/i.test(ua) || (/Macintosh/.test(ua) && (navigator.maxTouchPoints || 0) > 1);
    }
    // Yerel yardımcıya ver: program açık değilse bağlantı hemen reddedilir (false). Chrome ilk seferde "bu cihazdaki
    // uygulamalara erişim" izni sorabilir; yanıt beklenirken süre dolmasın diye uzun tutulur.
    async function yardimciyaKopyala(dosya) {
        const ctl = typeof AbortController === 'function' ? new AbortController() : null;
        const zaman = ctl ? setTimeout(function() { ctl.abort(); }, 90000) : null;
        try {
            const r = await fetch(YARDIMCI + '/__yardimci__/paylas', { method: 'POST', body: dosya, cache: 'no-store', mode: 'cors',
                credentials: 'omit', signal: ctl ? ctl.signal : undefined, headers: { 'X-Cu-Ad': b64(dosya.name) } });
            return r.ok && (await r.text()).trim() === 'pano';
        } catch (x) { return false; }
        finally { if (zaman) clearTimeout(zaman); }
    }
    // pdfGetir: UDF gibi paylaş ekranının almadığı türde PDF'ini veren işlev (yoksa null).
    async function paylas(dosya, el, pdfGetir) {
        const bilgi = kartBilgisi(el);
        if (buBilgisayar()) {
            // Pencere yok: belge dosya olarak panoya konur; istenen yere Ctrl+V.
            await programlaPaylas(dosya, bilgi);
            fareUyarisi(dosya.name);
            return 'Kopyalandı ✓';
        }
        if (mobilMi()) {
            let f = dosya;
            if (!tarayiciPaylasabilir(f) && pdfGetir) f = await pdfGetir();
            if (tarayiciPaylasabilir(f)) { await tarayiciylaPaylas(f, bilgi); return; }
            indir(f);
            return 'İndirildi ✓';
        }
        if (await yardimciyaKopyala(dosya)) { fareUyarisi(dosya.name); return 'Kopyalandı ✓'; }
        indir(dosya);
        bildir('Belge indirildi. Bu bilgisayarda Çoklu UYAP programı açık olsaydı doğrudan panoya kopyalanırdı (Ctrl+V).');
        return 'İndirildi ✓';
    }

    // ── Görüntüleyiciler ve belge kaynakları ──
    // tip 'ude':  UDF okuyucusu (.ude-document-viewer-toolbar). UYAP düğmeleri: title="İndir" (→ <a download>.udf),
    //             title="PDF Olarak İndir" (→ showSaveFilePicker .pdf). Canlı sayfada doğrulandı 17.11.2026.
    // tip 'pdf':  PDF görüntüleyicisi (.documentViewerHeader + react-pdf). Yalnız PDF düğmeleri (kullanıcı isteği).
    // tip 'kelime': öbür görüntüleyici (.documentViewerHeader, aria-label="show_download").
    function idbKayit(id) {
        return new Promise(function(coz) {
            if (!id) { coz(null); return; }
            let r;
            try { r = indexedDB.open('documentViewer'); } catch (x) { coz(null); return; }
            r.onupgradeneeded = function() { try { r.transaction.abort(); } catch (x) {} coz(null); };
            r.onerror = function() { coz(null); };
            r.onsuccess = function() {
                const db = r.result;
                if (!db.objectStoreNames.contains('documents')) { db.close(); coz(null); return; }
                const g = db.transaction('documents', 'readonly').objectStore('documents').get(id);
                g.onsuccess = function() { db.close(); coz(g.result && g.result.data && g.result.data.content ? g.result.data : null); };
                g.onerror = function() { db.close(); coz(null); };
            };
        });
    }
    function udeKaynak(cubuk) {
        const dl = Array.from(cubuk.querySelectorAll(':scope > button'));
        const baslikEl = cubuk.querySelector(':scope > span');
        return {
            tip: 'ude', kapEl: cubuk,
            orjDugme: dl.find(function(b) { return b.title === 'İndir'; }) || null,
            pdfDugme: dl.find(function(b) { return /^PDF/i.test(b.title || ''); }) || null,
            pdfVeri: null, yerelId: '',
            baslik: temizAd(baslikEl && baslikEl.textContent || 'belge'),
        };
    }
    function basKaynak(bas) {
        const kok = bas.parentElement || bas;
        const yDl = Array.from(bas.querySelectorAll('[aria-label="show_download"]'));
        const vp = propBul(bas, function(p) { return typeof p.localDocumentId === 'string' || typeof p.title === 'string' && 'headerVisible' in p; }, 14);
        let pdfVeri = null;
        const doc = kok.querySelector('.react-pdf__Document');
        if (doc) { const p = propBul(doc, function(p) { return typeof p.file === 'string' && p.file.indexOf('data:application/pdf') === 0; }, 10); if (p) pdfVeri = p.file; }
        const baslikEl = bas.querySelector('h6');
        const baslik = temizAd((vp && vp.title) || (baslikEl && baslikEl.textContent) || 'belge');
        if (pdfVeri || doc) return { tip: 'pdf', kapEl: bas, orjDugme: null, pdfDugme: null, pdfVeri: pdfVeri, yerelId: vp && vp.localDocumentId || '', baslik: baslik };
        return {
            tip: 'kelime', kapEl: bas,
            orjDugme: yDl.find(function(b) { return dugmeTuru(b) === 'orijinal'; }) || bas.querySelector('[aria-label="download"]') || null,
            pdfDugme: yDl.find(function(b) { return dugmeTuru(b) === 'pdf'; }) || null,
            pdfVeri: null, yerelId: vp && vp.localDocumentId || '', baslik: baslik,
        };
    }
    // Önbellek YALNIZ içerikle eşlenebiliyorsa (PDF verisi / yerel depo kimliği). Aynı adlı iki belge karışıp
    // yanlış belge paylaşılmasın diye UDF okuyucusunda önbellek yok: her tık belgeyi UYAP'tan yeniden alır.
    const onbellek = new Map();
    function durum(k) {
        const a = k.pdfVeri ? 'p' + k.pdfVeri.length + ':' + k.pdfVeri.slice(-64) : k.yerelId ? 'y' + k.yerelId : '';
        if (!a) return { orj: null, pdf: null, idb: null, idbBakildi: true, gecici: true };
        let d = onbellek.get(a);
        if (!d) { d = { orj: null, pdf: null, idb: null, idbBakildi: false }; onbellek.set(a, d); if (onbellek.size > 30) onbellek.delete(onbellek.keys().next().value); }
        return d;
    }
    function idbUzanti(kayit) {
        if (!kayit) return '';
        return uzanti(kayit.fileName) || MIME_UZANTI[(kayit.mimeType || '').toLowerCase()] || '';
    }
    function kayittanDosya(kayit, varsayilanAd) {
        const u = idbUzanti(kayit);
        let ad = temizAd(kayit.fileName || varsayilanAd);
        if (!uzanti(ad) && u) ad += '.' + u;
        const icerik = typeof kayit.content === 'string' ? kayit.content : new Uint8Array(kayit.content);
        return new File([icerik], ad, { type: tur(ad, kayit.mimeType) });
    }
    function pdfMi(f) { return uzanti(f.name) === 'pdf'; }
    function orjUzanti(k, d) {
        if (d.orj) return uzanti(d.orj.f.name);
        if (k.tip === 'ude') return 'udf';
        const u = idbUzanti(d.idb);
        return u && u !== 'pdf' ? u : '';
    }
    function orjVar(k, d) {
        if (k.tip === 'pdf') return false;
        if (d.orj && pdfMi(d.orj.f)) return false;
        return !!(k.orjDugme || (d.idb && idbUzanti(d.idb) !== 'pdf'));
    }
    function pdfVar(k, d) {
        if (k.tip === 'pdf') return !!k.pdfVeri || !!(d.idb && idbUzanti(d.idb) === 'pdf');
        return !!(k.pdfDugme || orjUzanti(k, d) === 'udf' || (d.orj && pdfMi(d.orj.f)) || (!orjUzanti(k, d) && k.orjDugme));
    }
    // Dosya adı UYAP'ın verdiği addan DEĞİL, görüntüleyici başlığından: UYAP sunucusunun adı Türkçe harfleri
    // bozuyor (canlıda: "Reddiyat_Beyan�_09_10_2026.pdf"). Biçim: "2026-8116 Belge Başlığı.pdf"
    function adlandir(f, k) {
        const u = uzanti(f.name) || (f.type === 'application/pdf' ? 'pdf' : '');
        let no = '';
        try {
            const kart = k.kapEl && k.kapEl.closest && k.kapEl.closest('.dosya-sorgula-popup');
            const v = kart && window.__uyapKartVerisi ? window.__uyapKartVerisi(kart) : null;
            no = v && v.dosyaNo ? String(v.dosyaNo).replace(/[\\/]+/g, '-') : '';
        } catch (x) {}
        const bas = k.baslik && k.baslik !== 'belge' ? k.baslik : f.name.replace(/\.[a-z0-9]+$/i, '').replace(/�/g, '');
        const ad = temizAd((no && bas.indexOf(no) < 0 ? no + ' ' : '') + bas) + (u ? '.' + u : '');
        return ad === f.name ? f : new File([f], ad, { type: f.type || tur(ad) });
    }
    // Portal belgelerinde (ör. Avukat Portal Genel Talebi) alanlar metinde ADIYLA durur, değerler UDF içindeki
    // ayrı <data> bölümünde; UYAP'ın ekranı birleştirir, başka her şey alan adını gösterir. Program değerleri
    // metne yerleştirir (udf_alan.rs). İçerik değiştiği için bu kopyada e-imza yoktur; imzalı asıl UYAP'ın
    // kendi "İndir" düğmesinde. Program yoksa ya da alan yoksa dosya olduğu gibi kalır.
    let doldurmaBildirildi = false;
    async function alanlariDoldur(f) {
        if (uzanti(f.name) !== 'udf') return f;
        try {
            const r = await fetch('/__uyap_yerel__/udf_doldur', { method: 'POST', body: f, credentials: 'include', cache: 'no-store' });
            if (r.status !== 200) return f;
            const dolu = new File([await r.blob()], f.name, { type: f.type || tur(f.name) });
            if (!doldurmaBildirildi && window.__uyapBildirim) {
                doldurmaBildirildi = true;
                window.__uyapBildirim('Alan değerleri belgeye yerleştirildi. Bu kopyada e-imza yok; imzalı asıl için UYAP\'ın kendi "İndir" düğmesini kullanın.');
            }
            return dolu;
        } catch (x) { return f; }
    }
    async function orjAl(k, d) {
        if (d.orj) return d.orj;
        let s = null;
        if (k.orjDugme) s = await yakala(k.orjDugme);
        else if (d.idb && idbUzanti(d.idb) !== 'pdf') s = { f: kayittanDosya(d.idb, k.baslik) };
        if (!s) throw new Error('Bu belgenin aslı alınamadı.');
        const o = { f: adlandir(await alanlariDoldur(s.f), k) };
        if (!d.gecici) d.orj = o;
        return o;
    }
    async function pdfAl(k, d) {
        if (d.pdf) return d.pdf;
        let f = null;
        if (k.pdfVeri) {
            const b = await (await fetch(k.pdfVeri)).blob();
            f = new File([b], temizAd(k.baslik) + '.pdf', { type: 'application/pdf' });
        } else if (k.pdfDugme) {
            f = (await yakala(k.pdfDugme)).f;
            if (!pdfMi(f)) f = new File([f], f.name.replace(/\.[a-z0-9]+$/i, '') + '.pdf', { type: 'application/pdf' });
        } else if (d.idb && idbUzanti(d.idb) === 'pdf') {
            f = kayittanDosya(d.idb, k.baslik);
        } else {
            const o = await orjAl(k, d);
            if (pdfMi(o.f)) f = o.f;
            else if (uzanti(o.f.name) === 'udf') {
                const r = await fetch('/__uyap_yerel__/udf_pdf', { method: 'POST', body: o.f, credentials: 'include', cache: 'no-store' });
                if (!r.ok) throw new Error('PDF\'e çevrilemedi.');
                f = new File([await r.blob()], o.f.name.replace(/\.[a-z0-9]+$/i, '') + '.pdf', { type: 'application/pdf' });
            } else throw new Error('Bu belgenin PDF hâli yok.');
        }
        const p = { f: adlandir(f, k) };
        if (!d.gecici) d.pdf = p;
        return p;
    }

    // ── Araç çubuğu düğmeleri ──
    function stilKur() {
        if (document.getElementById('cu-belge-dugme-stil') || !document.head) return;
        const s = document.createElement('style');
        s.id = 'cu-belge-dugme-stil';
        s.textContent = '[aria-label="sign_info"]{display:none!important}'
            + '.cu-bh [aria-label="download"],.cu-bh [aria-label="show_download"],[data-cu-gizle]{display:none!important}'
            + '.cu-bk{display:flex;align-items:center;gap:8px;margin:0 6px;flex:none;flex-wrap:nowrap}'
            + '.cu-bk-grup{display:flex;align-items:center;gap:4px}'
            + '.cu-bk-grup[hidden]{display:none}'
            + '.cu-bk-ayrac{width:1px;height:22px;background:var(--cu-t-kenar,rgba(0,0,0,.15))}'
            + '.cu-bk button{display:inline-flex;align-items:center;gap:5px;height:30px;padding:0 11px;border-radius:8px;cursor:pointer;white-space:nowrap;'
            + 'font:600 12.5px/1 -apple-system,"Segoe UI",Roboto,Arial,sans-serif;transition:filter .1s,background .1s;'
            + 'color:var(--cu-vurgu-koyu,var(--cu-vurgu,#1d4ed8));background:var(--cu-t-yuzey,#fff);border:1px solid var(--cu-vurgu-kenar,rgba(var(--cu-vurgu-rgb,37,99,235),.45))}'
            + '.cu-bk button:hover{background:var(--cu-vurgu-zemin,rgba(var(--cu-vurgu-rgb,37,99,235),.1))}'
            + '.cu-bk button.cu-bk-paylas{color:var(--cu-vurgu-uzeri,#fff);background:var(--cu-vurgu,#2563eb);border-color:var(--cu-vurgu,#2563eb)}'
            + '.cu-bk button.cu-bk-paylas:hover{filter:brightness(1.1)}'
            + '.cu-bk button.cu-bk-tamam,.cu-bk button.cu-bk-tamam:disabled{color:var(--cu-vurgu-yazi,#fff);background:var(--cu-vurgu-koyu,var(--cu-vurgu,#1d4ed8));border-color:var(--cu-vurgu-koyu,var(--cu-vurgu,#1d4ed8));opacity:1}'
            + '.cu-bk button:focus-visible{outline:2px solid var(--cu-vurgu,#2563eb);outline-offset:2px}'
            + '.cu-bk button[disabled]{opacity:.6;cursor:progress;filter:none}'
            + '.cu-bk svg{flex:none}'
            + '#cu-pano-uyari{position:fixed;z-index:2147483647;max-width:340px;border-radius:12px;overflow:hidden;cursor:default;'
            + 'background:var(--cu-t-yuzey,#fff);color:var(--cu-t-yazi,#1f2937);border:1px solid var(--cu-vurgu-kenar,rgba(var(--cu-vurgu-rgb,37,99,235),.45));'
            + 'box-shadow:0 10px 30px rgba(0,0,0,.22),0 2px 6px rgba(0,0,0,.12);font:13px/1.4 -apple-system,"Segoe UI",Roboto,Arial,sans-serif;'
            + 'opacity:0;transform:translateY(-6px) scale(.97);transition:opacity .2s,transform .2s}'
            + '#cu-pano-uyari.cu-pu-acik{opacity:1;transform:none}'
            + '#cu-pano-uyari .cu-pu-ic{display:flex;gap:11px;align-items:flex-start;padding:12px 14px}'
            + '#cu-pano-uyari .cu-pu-simge{flex:none;width:28px;height:28px;border-radius:50%;background:var(--cu-vurgu,#2563eb);color:var(--cu-vurgu-uzeri,#fff);'
            + 'display:flex;align-items:center;justify-content:center;font-weight:700;font-size:16px}'
            + '#cu-pano-uyari .cu-pu-metin{display:flex;flex-direction:column;gap:3px;min-width:0}'
            + '#cu-pano-uyari b{font-size:14px;color:var(--cu-vurgu-koyu,var(--cu-vurgu,#1d4ed8))}'
            + '#cu-pano-uyari .cu-pu-ad{font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}'
            + '#cu-pano-uyari kbd{display:inline-block;padding:0 5px;border:1px solid rgba(0,0,0,.25);border-bottom-width:2px;border-radius:4px;'
            + 'font:600 11.5px/1.5 Consolas,monospace;background:rgba(0,0,0,.04)}'
            + '#cu-pano-uyari .cu-pu-sure{height:3px;background:var(--cu-vurgu,#2563eb);transform-origin:left;transform:scaleX(1)}';
        document.head.appendChild(s);
    }
    function svg(d) { return '<svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true"><path fill="currentColor" d="' + d + '"></path></svg>'; }
    function dugme(sinif, yol) {
        const b = document.createElement('button');
        b.type = 'button';
        b.className = sinif;
        b.innerHTML = svg(yol) + '<span></span>';
        return b;
    }
    function yaz(b, metin) { const s = b.querySelector('span'); if (s.textContent !== metin) s.textContent = metin; }
    function calis(b, metin, is) {
        return async function(ev) {
            ev.preventDefault(); ev.stopPropagation();
            if (b.disabled) return;
            const eski = b.querySelector('span').textContent;
            b.disabled = true;
            yaz(b, metin);
            let sonuc;
            try { sonuc = await is(); }
            catch (e) { bildir(e && e.message ? e.message : 'İşlem yapılamadı.'); }
            if (typeof sonuc === 'string') {   // ör. "Kopyalandı ✓": düğmede kısa süre görünür
                yaz(b, sonuc); b.classList.add('cu-bk-tamam');
                await new Promise(function(c) { setTimeout(c, 2500); });
                b.classList.remove('cu-bk-tamam');
            }
            b.disabled = false; yaz(b, eski);
        };
    }
    // kaynak(): o anki kaynaklar (belge değişebilir, her tıkta yeniden okunur)
    function cubukYap(kaynak, kapEl) {
        const c = document.createElement('div');
        c.className = 'cu-bk';
        const g1 = document.createElement('div'); g1.className = 'cu-bk-grup cu-bk-orj';
        const g2 = document.createElement('div'); g2.className = 'cu-bk-grup cu-bk-pdf';
        const oi = dugme('cu-bk-indir', INDIR_SVG), op = dugme('cu-bk-paylas', PAYLAS_SVG);
        const pi = dugme('cu-bk-indir', INDIR_SVG), pp = dugme('cu-bk-paylas', PAYLAS_SVG);
        yaz(oi, 'UDF İndir'); yaz(op, 'UDF Paylaş'); yaz(pi, 'PDF İndir'); yaz(pp, 'PDF Paylaş');
        const kopyalar = !mobilMi();
        oi.title = 'Belgenin aslını kaydet';
        op.title = kopyalar ? 'Belgenin aslını kopyalar — WhatsApp, Outlook, klasör… nereye isterseniz Ctrl+V ile yapıştırın'
            + (buBilgisayar() ? '' : ' (bu bilgisayarda Çoklu UYAP açık değilse indirilir)') : 'Belgeyi paylaş (UDF ise PDF olarak)';
        pi.title = 'PDF olarak kaydet';
        pp.title = kopyalar ? 'PDF olarak kopyalar — WhatsApp, Outlook, klasör… nereye isterseniz Ctrl+V ile yapıştırın'
            + (buBilgisayar() ? '' : ' (bu bilgisayarda Çoklu UYAP açık değilse indirilir)') : 'PDF olarak paylaş';
        // Armonia Yazı (enjekte/yazi.js): UDF belgesini UYAP ekranının yanındaki editör penceresinde açar.
        const oy = dugme('cu-bk-indir cu-bk-yazi', YAZI_SVG);
        yaz(oy, "Yazı'da aç");
        oy.title = "Armonia Yazı'da aç: belge yanında açılır, okurken yazabilirsiniz (Alt+1…0 çoklu pano)";
        oy.addEventListener('click', calis(oy, 'Açılıyor…', async function() {
            const [x, d] = k();
            if (window.__cuYaziDosyaAc) {
                // Belgenin dosyası (kart verisi): editör o dosyaya bağlanır, imzalanan belge oraya gönderilebilir
                let v = null;
                try { const kart = x.kapEl && x.kapEl.closest && x.kapEl.closest('.dosya-sorgula-popup'); v = kart && window.__uyapKartVerisi ? window.__uyapKartVerisi(kart) : null; } catch (e) {}
                window.__cuYaziDosyaAc((await orjAl(x, d)).f, v);
            }
        }));
        g1.appendChild(oi); g1.appendChild(op); g1.appendChild(oy); g2.appendChild(pi); g2.appendChild(pp);
        const ay = document.createElement('span'); ay.className = 'cu-bk-ayrac';
        c.appendChild(g1); c.appendChild(ay); c.appendChild(g2);
        const k = function() { const x = kaynak(); return [x, durum(x)]; };
        oi.addEventListener('click', calis(oi, 'Hazırlanıyor…', async function() { const [x, d] = k(); seciciyleKaydet((await orjAl(x, d)).f); }));
        op.addEventListener('click', calis(op, 'Hazırlanıyor…', async function() {
            const [x, d] = k();
            return paylas((await orjAl(x, d)).f, kapEl, async function() { return (await pdfAl(x, d)).f; });
        }));
        pi.addEventListener('click', calis(pi, 'Hazırlanıyor…', async function() { const [x, d] = k(); seciciyleKaydet((await pdfAl(x, d)).f); }));
        pp.addEventListener('click', calis(pp, 'Hazırlanıyor…', async function() { const [x, d] = k(); return paylas((await pdfAl(x, d)).f, kapEl); }));
        return c;
    }
    function cubukGuncelle(c, k) {
        const d = durum(k);
        if (!d.idbBakildi && k.yerelId) {
            d.idbBakildi = true;
            idbKayit(k.yerelId).then(function(r) { d.idb = r; });
        }
        const g1 = c.querySelector('.cu-bk-orj'), g2 = c.querySelector('.cu-bk-pdf');
        const ov = orjVar(k, d), pv = pdfVar(k, d);
        g1.hidden = !ov; g2.hidden = !pv;
        c.querySelector('.cu-bk-ayrac').style.display = ov && pv ? '' : 'none';
        const et = etiketi(orjUzanti(k, d));
        const [oi, op] = g1.querySelectorAll('button');
        const oy = g1.querySelector('.cu-bk-yazi');
        if (oy) oy.hidden = !(window.__cuYaziDosyaAc && orjUzanti(k, d) === 'udf');
        if (!oi.disabled) yaz(oi, et + ' İndir');
        if (!op.disabled) yaz(op, et + ' Paylaş');
        c.style.display = ov || pv ? '' : 'none';
    }
    function cubuklar() {
        // UDF okuyucusu
        document.querySelectorAll('.ude-document-viewer-toolbar').forEach(function(tb) {
            const k = udeKaynak(tb);
            if (!k.orjDugme && !k.pdfDugme) return;
            if (k.orjDugme) k.orjDugme.setAttribute('data-cu-gizle', '1');
            if (k.pdfDugme) k.pdfDugme.setAttribute('data-cu-gizle', '1');
            let c = tb.querySelector(':scope > .cu-bk');
            if (!c) {
                c = cubukYap(function() { return udeKaynak(tb); }, tb);
                const ayrac = tb.querySelector(':scope > span + div');   // başlık + ayraç
                if (ayrac) ayrac.insertAdjacentElement('afterend', c); else tb.insertBefore(c, tb.firstChild);
            }
            cubukGuncelle(c, k);
        });
        // PDF görüntüleyicisi ve öbür görüntüleyici
        document.querySelectorAll('.documentViewerHeader').forEach(function(bas) {
            const k = basKaynak(bas);
            if (!k.orjDugme && !k.pdfDugme && !k.pdfVeri && !k.yerelId) return;   // belgesiz başlık
            bas.classList.add('cu-bh');
            let c = bas.querySelector('.cu-bk');
            if (!c) {
                const sol = bas.querySelector('.MuiStack-root .MuiStack-root') || bas.querySelector('.MuiStack-root');
                if (!sol) return;
                c = cubukYap(function() { return basKaynak(bas); }, bas);
                const ayrac = sol.querySelector(':scope > [role="separator"]');
                if (ayrac) ayrac.insertAdjacentElement('afterend', c); else sol.insertBefore(c, sol.firstChild);
            }
            cubukGuncelle(c, k);
        });
    }

    function imzaPenceresiniKapat() {
        document.querySelectorAll('.dx-popup-wrapper').forEach(function(w) {
            if (w.dataset.cuImza || w.style.display === 'none') return;
            const b = w.querySelector('.dx-popup-title, .dx-toolbar');
            if (b && /(İmza|Mühür) Doğrulan/.test(b.textContent || '')) { w.dataset.cuImza = '1'; w.style.display = 'none'; }
        });
    }

    function tur_() { try { stilKur(); imzaPenceresiniKapat(); cubuklar(); } catch (x) {} }
    setInterval(tur_, 700);
    tur_();
})();
