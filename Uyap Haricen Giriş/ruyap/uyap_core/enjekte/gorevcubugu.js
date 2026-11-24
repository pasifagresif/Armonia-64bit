// ═══════════════════════════════════════════════════════════════════════════════════════
// ÇOKLU UYAP — GÖREV ÇUBUĞU, BAŞLAT DÜĞMESİ ve genel arayüz düzeni (2026-11-11)
// ═══════════════════════════════════════════════════════════════════════════════════════
// uyap_proxy._rewrite_body bu dosyayı ana enjekte betiğin ardından satır içi ekler; içeriği
// INJECT_VERSION'a katılır.
//
// • Görev çubuğu: ekranın altında, tam genişlik, UYAP ekranında her zaman görünür. Girdileri dışarıdan
//   verilir (kartlar.js: küçültülen UYAP dosya kartları; icra.js: İcra Takip Süreçleri sayfası) — API:
//   window.CU.gorev.ayarla([{kimlik, ad, ikon, on, kucuk, rozet, tikla(), kapat(), menu()}], kaynak).
//   Boşken bir ipucu yazısı durur.
// • "ÇOKLU UYAP" başlat düğmesi: UYAP'ın sol menüsünü açar/kapatır. Varsayılan kapalı; açılınca görev
//   çubuğundan yukarı kayar ve sayfayı iter (üstüne binmez). Menü üst çubuğun altından başlar, logo yok.
// • Kullanıcı kararıyla gizlenen UYAP öğeleri (her sayfada): alt bilgi çubuğu, erişilebilirlik düğmesi ve
//   paneli, UYAP'ın kendi menü daraltma düğmesi.
//
// Yerleşim (canlı ölçüm, 2026-11-11): sol menü #av-sidebar-wrapper (sabit, 290 px, z 1500), üst çubuk
// .u-navbar (60 px), içerik #page-content-wrapper (margin-left ile itilir). UYAP'ın açılır pencereleri
// (DevExtreme) z ≥ 1503: görev çubuğu (1501) onların altında; kart görev çubuğuna yer bırakır (kartlar.js).
//
// Dar ekran / telefon (mobil.js kip sınıfları, kurallar mobil.css): sol menü sayfayı İTMEZ, üstüne açılır
// (arkası #cu-sol-perde ile kararır; dışına ya da bir bağlantıya dokununca kapanır), UYAP'ın üst çubuktaki ☰
// düğmesi de bu menüyü açar. Telefonda başlat düğmesi yalnız ☰, dosya düğmeleri iki satırlı (dosya no / birim)
// ve yana kayar. Çubuğun yüksekliği --cu-cubuk-yuk (mobil.css: 44 px, telefonda 52 px + alt güvenli alan).
(function() {
    'use strict';
    const anaCerceve = window.__uyapAnaCerceve ? window.__uyapAnaCerceve() : window.top === window;
    if (!anaCerceve || window.CU) return;

    const YUK = 'var(--cu-cubuk-yuk,44px)';     // görev çubuğu yüksekliği: mobil.css (44 px; telefonda 52 px)
    let cubuk = null, cubukListe = null, perde = null, stilKuruldu = false, solAcik = false;
    const gorevler = [];
    const dar = function() { return !!(window.__cuDar && window.__cuDar()); };

    // Programın önceki sürümlerinden (vazgeçilen kendi pencere / dosya ekranı denemeleri) tarayıcıda kalan
    // veriler: bir kez silinir.
    try {
        if (!localStorage.getItem('cokluuyapTemizlik1')) {
            ['cokluuyapPencereler', 'cokluuyapBelgeDizini'].forEach(function(k) { localStorage.removeItem(k); });
            if (window.indexedDB) indexedDB.deleteDatabase('cokluuyap');
            localStorage.setItem('cokluuyapTemizlik1', '1');
        }
    } catch (e) {}

    function el(tag, sinif, metin) {
        const e = document.createElement(tag);
        if (sinif) e.className = sinif;
        if (metin != null) e.textContent = metin;
        return e;
    }
    function uyapHazir() { return !!document.getElementById('av-sidebar-wrapper'); }

    function stilKur() {
        if (stilKuruldu) return;
        stilKuruldu = true;
        const s = document.createElement('style');
        s.textContent = [
            // Görev çubuğu
            '#uyap-gorev-cubugu{position:fixed;left:0;right:0;bottom:0;height:' + YUK + ';z-index:1501;display:none;align-items:stretch;',
            'background:linear-gradient(var(--cu-cubuk-ust),var(--cu-cubuk-alt));border-top:2px solid var(--cu-cubuk-vurgu);box-shadow:0 -4px 14px rgba(0,0,0,0.25);font:13px/1.3 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;color:var(--cu-cubuk-yazi)}',
            '#uyap-gorev-cubugu .cu-gc-liste{position:relative;flex:1;min-width:0;display:flex;align-items:center;gap:6px;padding:0 10px;overflow-x:auto;overflow-y:hidden;scrollbar-width:thin}',
            '#uyap-gorev-cubugu .cu-gc{flex:none;display:flex;align-items:center;gap:8px;max-width:260px;height:34px;padding:0 14px;border:1px solid rgba(255,255,255,0.10);',
            'border-radius:7px;background:rgba(255,255,255,0.07);color:var(--cu-cubuk-yazi);cursor:pointer;font:inherit;font-weight:500;white-space:nowrap;transition:background .12s}',
            '#uyap-gorev-cubugu .cu-gc:hover{background:rgba(255,255,255,0.16)}',
            '#uyap-gorev-cubugu .cu-gc.on{background:var(--cu-cubuk-vurgu);border-color:var(--cu-vurgu-parlak);color:var(--cu-vurgu-uzeri);font-weight:700;box-shadow:inset 0 -3px 0 var(--cu-vurgu-acik)}',
            '#uyap-gorev-cubugu .cu-gc.kucuk{color:var(--cu-cubuk-soluk)}',
            '#uyap-gorev-cubugu .cu-gc-ad{overflow:hidden;text-overflow:ellipsis}',
            // Telefonda iki satır: üstte dosya no, altta birim (mobil.css); masaüstünde tek satır tam ad.
            '#uyap-gorev-cubugu .cu-gc-ust,#uyap-gorev-cubugu .cu-gc-alt,#uyap-gorev-cubugu .cu-gc-ipucu .kisa{display:none}',
            '#uyap-gorev-cubugu .cu-gc-ikon{flex:none;font-size:15px}',
            '#uyap-gorev-cubugu .cu-gc-rozet{min-width:18px;padding:0 6px;border-radius:999px;background:#ef4444;color:#fff;font-size:11px;font-weight:700;line-height:18px;text-align:center}',
            '#uyap-gorev-cubugu .cu-gc-ipucu{padding:0 4px;color:var(--cu-cubuk-soluk);font-size:12.5px;white-space:nowrap}',
            // "ÇOKLU UYAP" başlat düğmesi (açıkken renk değişmez; yalnız basılı görünür ve ok döner)
            // Genişliği sol menüyle aynı (her iki yandan 6 px içeride): düğme ile menü alt alta simetrik durur.
            '#uyap-gorev-cubugu .cu-gc-marka{flex:none;display:flex;align-items:center;gap:9px;margin:4px 6px;padding:0 14px 0 8px;width:calc(var(--cu-sol-gen,290px) - 12px);box-sizing:border-box;',
            'border:1px solid rgba(255,255,255,0.22);border-radius:8px;background:linear-gradient(var(--cu-marka-ust),var(--cu-marka-alt));color:var(--cu-cubuk-yazi);cursor:pointer;',
            'font:800 12.5px/1 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;letter-spacing:.08em;white-space:nowrap;user-select:none;',
            'box-shadow:0 1px 0 rgba(255,255,255,0.18) inset,0 2px 6px rgba(0,0,0,0.3);transition:background .12s,transform .05s}',
            '#uyap-gorev-cubugu .cu-gc-marka:hover{background:linear-gradient(var(--cu-marka-ust-acik),var(--cu-marka-alt-acik))}',
            '#uyap-gorev-cubugu .cu-gc-marka:active{transform:translateY(1px)}',
            '#uyap-gorev-cubugu .cu-gc-marka.acik{background:linear-gradient(var(--cu-marka-basili),var(--cu-marka-alt));box-shadow:inset 0 2px 5px rgba(0,0,0,0.4)}',
            '#uyap-gorev-cubugu .cu-gc-marka:focus-visible{outline:2px solid var(--cu-vurgu-acik);outline-offset:1px}',
            '#uyap-gorev-cubugu .cu-gc-marka i{display:inline-flex;align-items:center;justify-content:center;width:26px;height:26px;border-radius:6px;background:rgba(255,255,255,0.16);font-style:normal;font-size:15px;letter-spacing:0}',
            '#uyap-gorev-cubugu .cu-gc-marka i.logo{background:none;width:28px;height:28px}',
            '#uyap-gorev-cubugu .cu-gc-marka i.logo img{display:block;width:28px;height:28px;border-radius:7px;box-shadow:0 1px 3px rgba(0,0,0,0.3)}',
            // Sol menünün tepesindeki boşluk (üst çubuk hizası): Armonia logosu + ad. Dar ekranda menü üst
            // çubuğun altından başladığı için orada boşluk yok, gizli.
            'body.cu-gorev #cu-menu-logo{position:absolute;left:0;right:0;top:0;height:var(--cu-nav-yuk,60px);display:flex;align-items:center;gap:10px;padding:0 18px;box-sizing:border-box;',
            'border-bottom:1px solid var(--cu-t-kenar,rgba(0,0,0,0.08));color:var(--cu-t-sol-yazi,#333);font:600 17px/1.05 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;user-select:none;pointer-events:none}',
            'body.cu-gorev #cu-menu-logo .cu-ml-i{display:block;flex:none;width:28px;height:28px}',
            'body.cu-gorev #cu-menu-logo .cu-ml-i img{display:block!important;width:28px;height:28px;border-radius:7px;box-shadow:0 1px 3px rgba(0,0,0,0.25)}',
            'body.cu-gorev #cu-menu-logo b{display:block;font-size:15px;font-weight:600;letter-spacing:0.01em}',
            'body.cu-gorev #cu-menu-logo small{display:block;margin-top:2px;font-size:9.5px;font-weight:500;letter-spacing:.08em;opacity:.75}',
            'html.cu-dar body.cu-gorev #cu-menu-logo{display:none}',
            '#uyap-gorev-cubugu .cu-gc-marka span.say{margin-left:2px;padding:0 7px;border-radius:999px;background:rgba(255,255,255,0.16);font-size:11px;font-weight:700;letter-spacing:0}',
            '#uyap-gorev-cubugu .cu-gc-marka .ok{margin-left:auto;font-size:10px;opacity:.85;letter-spacing:0;transition:transform .2s}',
            '#uyap-gorev-cubugu .cu-gc-marka.acik .ok{transform:rotate(180deg)}',
            // Gizlenen UYAP öğeleri: alt bilgi çubuğu, erişilebilirlik düğmesi + paneli, menü daraltma düğmesi
            'body.cu-gorev footer.footer{display:none!important}',
            'body.cu-gorev #accessibilityToggle,body.cu-gorev .ac-panel,body.cu-gorev [class*="ac-panel--"]{display:none!important}',
            'body.cu-gorev #av-sidebar-wrapper .sidebar-toggle-btn{display:none!important}',
            // Sol menü (kullanıcı kararları 2026-11-11): zemini tavandan görev çubuğuna kadar (üst çubuğun sol
            // ucunu da örter), öğeler üst çubuk hizasının altından başlar; logo yok (UYAP 'main-logo' sınıfını
            // menünün kapsayıcısına da verdiği için yalnız bağlantı gizlenir). 290 → 260 px; açılır menü oklarının
            // kutusu 35 → 20 px (okun sağındaki boşluk metnin solundakine yakın). Animasyonsuz açılır/kapanır.
            // Açılır liste kendi içinde kayar (UYAP overflow:hidden ile kesiyordu). Sık Kullanılanlar ile alt
            // menü arası ~33 px → ~12 px.
            'body.cu-gorev #av-sidebar-wrapper{top:0!important;bottom:auto!important;height:calc(100% - ' + YUK + ')!important;',
            'padding-top:calc(var(--cu-nav-yuk,60px) + 6px);width:260px!important;min-width:260px!important;max-width:260px!important;',
            'transform:translateY(calc(100% + ' + YUK + ' + 20px));transition:none!important}',
            // UYAP metin etiketine sabit genişlik veriyor, ok hemen arkasına diziliyordu (sağda ~44 px boşluk):
            // etiket esner, ok sağ kenara yaslanır → okun sağı ~31 px, metnin solu ~29 px (canlı ölçüm).
            'body.cu-gorev #av-sidebar-wrapper button.sideLink{display:flex!important;align-items:center;justify-content:space-between}',
            'body.cu-gorev #av-sidebar-wrapper .SidebarLabel{flex:1 1 auto!important;width:auto!important;min-width:0!important}',
            'body.cu-gorev #av-sidebar-wrapper .menu-arrow{width:20px!important;min-width:20px!important;flex:none!important;justify-content:flex-end!important;text-align:right}',
            'body.cu-gorev #av-sidebar-wrapper a.main-logo{display:none!important}',
            // Menü metinlerinin solundaki simgeler ve "Menüde Arama Yap" satırı: kullanıcı kararıyla gizli.
            // (Yalnız satır başındaki simge; sağdaki açılır-kapanır okları kalır.)
            'body.cu-gorev #av-sidebar-wrapper :is(a,button,li,span,div) > :is(i,svg,img):first-child:not(:only-child){display:none!important}',
            'body.cu-gorev #av-sidebar-wrapper > div > div:not([class]){display:none!important}',
            'body.cu-gorev #av-sidebar-wrapper .sidebar-top-part > ul:first-child{margin-top:4px!important}',
            'body.cu-gorev #av-sidebar-wrapper .favorite-menu-list{padding-bottom:4px!important}',
            'body.cu-gorev #av-sidebar-wrapper .favorite-menu-list > ul.menu-list{margin-bottom:8px!important}',
            'body.cu-gorev #av-sidebar-wrapper > div > ul.menu-list{flex:1 1 auto!important;min-height:0!important;max-height:none!important;height:auto!important;overflow-y:auto!important;overflow-x:hidden!important;scrollbar-width:thin}',
            'body.cu-gorev #page-content-wrapper{margin-left:0!important;transition:none!important}',
            // Açık: menü yerinde, sayfa itilir; UYAP dosya kartının (z 1503) da üstünde (kart kartlar.js'te yana yerleşir)
            'body.cu-gorev.cu-solacik #av-sidebar-wrapper{transform:none;z-index:1600!important}',
            'body.cu-gorev.cu-solacik #page-content-wrapper{margin-left:var(--cu-sol-gen,290px)!important}',
            // Dar ekranda açık menünün arkası (mobil.css gösterir): üst çubuğun altından görev çubuğuna kadar.
            '#cu-sol-perde{display:none;position:fixed;left:0;right:0;top:var(--cu-nav-yuk,60px);bottom:' + YUK + ';z-index:1599;background:rgba(0,0,0,0.42);touch-action:none}',
            // Katman sırası (bkz. katmanDuzelt): Bootstrap modalları da sol menünün (1600) önünde açılsın.
            'body.cu-gorev .modal-backdrop{z-index:' + (KATMAN_DIYALOG - 5) + '!important}',
            'body.cu-gorev .modal{z-index:' + KATMAN_DIYALOG + '!important}',
        ].join('');
        document.head.appendChild(s);
    }

    // ── Katman sırası (kullanıcı isteği 2026-11-14) ──────────────────────────────────────────────────
    // sayfa < görev çubuğu (1501) < UYAP dosya kartı (≤ 1590) < sol menü (1600) < diğer UYAP pencereleri.
    // DevExtreme her açılan pencereye 1500'den artan z-index verir: "Evrak gönder" gibi pencereler açık sol
    // menünün (1600) ARKASINDA kalıyordu; telefonda da birikmiş z-index'li dosya kartı menünün ÖNÜNE
    // geçebiliyordu. Kart menünün altına sıkıştırılır, diğer pencereler göreli sıraları korunarak
    // menünün üstüne taşınır (z + 200).
    const KATMAN_KART_UST = 1590, KATMAN_DIYALOG = 1660;
    let katmanBekliyor = false;
    function katmanDuzelt() {
        katmanBekliyor = false;
        if (!document.body || !document.body.classList.contains('cu-gorev')) return;
        document.querySelectorAll('.dx-overlay-wrapper').forEach(function(w) {
            const z = parseInt(w.style.zIndex, 10);
            if (!z) return;
            if (w.classList.contains('dosya-sorgula-popup')) {
                if (z > KATMAN_KART_UST) w.style.zIndex = String(KATMAN_KART_UST);
            } else if (z < 1610) {
                w.style.zIndex = String(z + 200);
            }
        });
    }
    function katmanPlanla() {
        if (katmanBekliyor) return;
        katmanBekliyor = true;
        requestAnimationFrame(katmanDuzelt);
    }
    function katmanIzle() {
        if (!document.body) { document.addEventListener('DOMContentLoaded', katmanIzle, { once: true }); return; }
        // DevExtreme pencereyi gösterirken sarmalayıcıyı ekler ve z-index'i style ile yazar.
        new MutationObserver(katmanPlanla).observe(document.body, {
            childList: true, subtree: true, attributes: true, attributeFilter: ['style'],
        });
        katmanPlanla();
    }
    katmanIzle();

    function solMenu(ac) {
        solAcik = !!ac;
        // Dar ekranda açık kullanıcı şeridi (tarih, tema, kullanıcı) üst çubuğu uzatır; menü onun altından başlasın
        // diye önce o kapatılır.
        if (solAcik && dar() && document.querySelector('.u-navbar .navbar-collapse.show')) {
            const b = document.getElementById('navHeaderContent');
            if (b) b.click();
        }
        const yan = document.getElementById('av-sidebar-wrapper');
        if (yan) document.documentElement.style.setProperty('--cu-sol-gen', yan.offsetWidth + 'px');
        // --cu-nav-yuk artık CSS'te (tema.css, mobil.css: yatık telefonda daha ince); eski sürümlerin satır içi
        // ölçümü kalmışsa silinir, yoksa yön değişince eski yükseklik takılı kalır.
        document.documentElement.style.removeProperty('--cu-nav-yuk');
        document.body.classList.toggle('cu-solacik', solAcik);
        ciz();
        // Sayfa genişliği değişti: UYAP'ın tabloları/ızgaraları yeni genişliğe uysun (dar ekranda menü itmez).
        if (!dar()) setTimeout(function() { window.dispatchEvent(new Event('resize')); }, 30);
    }
    // Dar ekran: UYAP'ın üst çubuktaki ☰ düğmesi (kendi menüsünü açıp kapatıyordu; menü artık bizim düzenimizde,
    // düğme işlevsiz kalıyordu) sol menümüzü açar / kapatır. Simgesi mobil.css'te menünün durumuna bağlı.
    document.addEventListener('click', function(ev) {
        if (!dar() || !document.body || !document.body.classList.contains('cu-gorev')) return;
        const b = ev.target && ev.target.closest ? ev.target.closest('.u-navbar .navbar-toggler') : null;
        if (!b || !b.querySelector('.fa-bars, .fa-times')) return;
        ev.preventDefault();
        ev.stopPropagation();
        ev.stopImmediatePropagation();
        solMenu(!solAcik);
    }, true);
    // Dar ekran: menüden bir sayfaya gidilince menü kapanır (yıldız = sık kullanılanlara ekle / çıkar hariç).
    document.addEventListener('click', function(ev) {
        if (!solAcik || !dar() || !ev.target || !ev.target.closest) return;
        if (!ev.target.closest('#av-sidebar-wrapper a') || ev.target.closest('.fav-menu-icon')) return;
        setTimeout(function() { solMenu(false); }, 0);
    });

    function kur() {
        stilKur();
        cubuk = el('div');
        cubuk.id = 'uyap-gorev-cubugu';
        const marka = el('button', 'cu-gc-marka');
        marka.type = 'button';
        // Simge: Armonia logosu (kullanıcı isteği 2026-11-14); logo yoksa eski ☰.
        const markaIkon = el('i', window.__cuLogo ? 'logo' : null, window.__cuLogo ? null : '☰');
        if (window.__cuLogo) {
            const li = document.createElement('img');
            li.src = window.__cuLogo; li.alt = ''; li.draggable = false;
            markaIkon.appendChild(li);
        }
        marka.append(markaIkon, el('span', null, 'ÇOKLU UYAP'), el('span', 'say'), el('span', 'ok', '▲'));
        marka.title = 'UYAP menüsünü aç / kapat';
        marka.setAttribute('aria-label', 'UYAP menüsünü aç / kapat');
        marka.addEventListener('click', function(e) { e.stopPropagation(); solMenu(!solAcik); });
        cubukListe = el('div', 'cu-gc-liste');
        cubukListe.addEventListener('wheel', function(e) {      // tekerlek: yatay kaydır
            if (e.deltaY) { cubukListe.scrollLeft += e.deltaY; e.preventDefault(); }
        }, { passive: false });
        cubuk.append(marka, cubukListe);
        document.body.appendChild(cubuk);
        perde = el('div');
        perde.id = 'cu-sol-perde';
        perde.addEventListener('click', function() { solMenu(false); });
        document.body.appendChild(perde);
    }
    // Açılan (öne gelen) dosyanın düğmesi çubukta görünür yere kaydırılır (telefonda liste yana kayar).
    function gorunureKaydir(d) {
        const sol = d.offsetLeft - 8, sag = d.offsetLeft + d.offsetWidth + 8 - cubukListe.clientWidth;
        if (cubukListe.scrollLeft > sol) cubukListe.scrollLeft = Math.max(0, sol);
        else if (cubukListe.scrollLeft < sag) cubukListe.scrollLeft = sag;
    }

    // Menü başlığındaki logo, üst çubuktaki logomuzla piksel piksel aynı yerde dursun (kullanıcı isteği
    // 2026-11-14): menü açılınca yalnız arka plan değişiyormuş gibi görünür. Üst çubuk logosunun gerçek
    // konumu ölçülür, başlık ona göre kaydırılır (menü sol kenarı 0'da).
    function logoHizala() {
        const ust = document.querySelector('.u-navbar .cu-logo-isaret');
        const bas = document.getElementById('cu-menu-logo');
        if (!ust || !bas) return;
        const r = ust.getBoundingClientRect();
        if (!r.width) return;
        const yan = document.getElementById('av-sidebar-wrapper');
        const sol = yan ? yan.getBoundingClientRect().left : 0;
        const x = Math.max(0, Math.round(r.left - sol)) + 'px';
        if (bas.style.paddingLeft !== x) bas.style.paddingLeft = x;
        // Dikey: başlık üst çubuk yüksekliğinde ve ortalı; simge merkezi farklıysa iç payla kaydır.
        const h = bas.offsetHeight || 60;
        const fark = Math.round((r.top + r.height / 2) - h / 2);
        const ust_ = fark > 0 ? 2 * fark + 'px' : '0px', alt_ = fark < 0 ? -2 * fark + 'px' : '0px';
        if (bas.style.paddingTop !== ust_) bas.style.paddingTop = ust_;
        if (bas.style.paddingBottom !== alt_) bas.style.paddingBottom = alt_;
    }

    function ciz() {
        if (!document.body) return;
        if (!cubuk) kur();
        const yan = document.getElementById('av-sidebar-wrapper');
        if (yan && window.__cuLogo && !document.getElementById('cu-menu-logo')) {
            const b = el('div');
            b.id = 'cu-menu-logo';
            const i = el('span', 'cu-ml-i');
            const im = document.createElement('img');
            im.src = window.__cuLogo; im.alt = ''; im.draggable = false;
            i.appendChild(im);
            const ad = el('span');
            ad.append(el('b', null, 'Çoklu UYAP'), el('small', null, 'BÜRO PANELİ'));
            b.append(i, ad);
            yan.insertBefore(b, yan.firstChild);
        }
        logoHizala();
        if (yan && yan.offsetWidth) document.documentElement.style.setProperty('--cu-sol-gen', yan.offsetWidth + 'px');
        const gorunur = uyapHazir();
        cubuk.style.display = gorunur ? 'flex' : 'none';
        const bosluk = gorunur ? YUK : '';                  // sayfanın sonu çubuğun arkasında kalmasın
        if (document.body.style.paddingBottom !== bosluk) document.body.style.paddingBottom = bosluk;
        document.body.classList.toggle('cu-gorev', gorunur);
        if (!gorunur) { document.body.classList.remove('cu-solacik'); return; }
        const marka = cubuk.firstChild;
        marka.classList.toggle('acik', solAcik);
        marka.setAttribute('aria-expanded', solAcik ? 'true' : 'false');
        const mevcut = {};
        Array.from(cubukListe.children).forEach(function(d) { mevcut[d.getAttribute('data-kimlik')] = d; });
        gorevler.forEach(function(g, i) {
            let d = mevcut[g.kimlik];
            delete mevcut[g.kimlik];
            if (!d) {
                d = el('button', 'cu-gc');
                d.type = 'button';
                d.setAttribute('data-kimlik', g.kimlik);
                const adKabi = el('span', 'cu-gc-ad');
                adKabi.append(el('span', 'cu-gc-tam'), el('span', 'cu-gc-ust'), el('span', 'cu-gc-alt'));
                d.append(el('span', 'cu-gc-ikon'), adKabi, el('span', 'cu-gc-rozet'));
                // Tıklama anında güncel girdi kullanılır (kapanış değişkeni bayatlamasın).
                const simdiki = function() { return gorevler.find(function(x) { return x.kimlik === g.kimlik; }); };
                d.addEventListener('click', function() { const x = simdiki(); if (x) x.tikla(); });
                d.addEventListener('auxclick', function(e) { if (e.button === 1) { e.preventDefault(); const x = simdiki(); if (x) x.kapat(); } });
                d.addEventListener('contextmenu', function(e) {
                    e.preventDefault();
                    e.stopPropagation();
                    const x = simdiki();
                    if (!x || !window.__uyapSagMenuAc) return;
                    const gruplar = [[[x.ad, null]].concat((x.menu ? x.menu() : [[]])[0] || []).concat([['Kapat', x.kapat]])];
                    gruplar.push([['Tümünü kapat', function() { gorevler.slice().forEach(function(y) { y.kapat(); }); }]]);
                    window.__uyapSagMenuAc(e.clientX, Math.max(4, e.clientY - 40 - 26 * (gruplar[0].length + 1)), gruplar);
                });
            }
            if (cubukListe.children[i] !== d) cubukListe.insertBefore(d, cubukListe.children[i] || null);
            const yeniAcildi = !!g.on && !d.classList.contains('on');
            d.classList.toggle('on', !!g.on);
            d.classList.toggle('kucuk', !!g.kucuk);
            d.classList.toggle('iki', !!g.ust);              // telefonda iki satır: g.ust (dosya no) / g.alt (birim)
            d.title = g.ad + (g.on ? ' (açık — tıklayınca küçülür)' : ' (tıklayınca açılır)') + '\nOrta tık / sağ tık (telefonda uzun basış): kapat';
            if (d.children[0].textContent !== g.ikon) d.children[0].textContent = g.ikon;
            const adlar = d.children[1].children;
            [g.ad, g.ust || '', g.alt || ''].forEach(function(m, j) { if (adlar[j].textContent !== m) adlar[j].textContent = m; });
            d.children[2].textContent = g.rozet || '';
            d.children[2].style.display = g.rozet ? '' : 'none';
            if (yeniAcildi) gorunureKaydir(d);
        });
        Object.keys(mevcut).forEach(function(k) { if (k !== 'ipucu') mevcut[k].remove(); });
        const say = marka.querySelector('.say');
        say.textContent = gorevler.length ? String(gorevler.length) : '';
        say.style.display = gorevler.length ? '' : 'none';
        let ipucu = mevcut.ipucu || cubukListe.querySelector('[data-kimlik="ipucu"]');
        if (!gorevler.length) {
            if (!ipucu) {
                ipucu = el('span', 'cu-gc-ipucu');
                ipucu.append(el('span', 'uzun', 'Dosya kartlarını başlıktaki “–” ile buraya küçültün; tek tıkla geri açılır.'),
                             el('span', 'kisa', 'Küçültülen dosyalar burada durur.'));
                ipucu.setAttribute('data-kimlik', 'ipucu');
                cubukListe.appendChild(ipucu);
            }
        } else if (ipucu) ipucu.remove();
    }

    setInterval(ciz, 1000);       // UYAP girişten çıktı / giriş yaptı: görünürlük güncellensin
    // Kip değişti (telefon çevrildi, pencere daraldı): menü ölçüleri yeniden alınsın.
    if (window.__cuKipDegisince) window.__cuKipDegisince(function() { if (solAcik) solMenu(true); else ciz(); });

    // Birden çok kaynak girdi verir (kartlar.js: dosya kartları, icra.js: İcra Takip Süreçleri sayfası); her kaynak
    // kendi listesini bütünüyle yeniler. Çubukta sıra: önce sayfalar ('icra'), sonra dosya kartları.
    const kaynaklar = {}, KAYNAK_SIRA = ['guncelleme', 'isler', 'icra', 'yazi', 'kart'];
    window.CU = {
        gorev: { ayarla: function(liste, kaynak) {
            kaynaklar[kaynak || 'kart'] = liste || [];
            const adlar = Object.keys(kaynaklar).sort(function(x, y) {
                const i = KAYNAK_SIRA.indexOf(x), j = KAYNAK_SIRA.indexOf(y);
                return (i < 0 ? 99 : i) - (j < 0 ? 99 : j);
            });
            gorevler.length = 0;
            adlar.forEach(function(k) { kaynaklar[k].forEach(function(g) { gorevler.push(g); }); });
            ciz();
        } },
    };
})();
