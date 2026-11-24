// ═══════════════════════════════════════════════════════════════════════════════════════
// ÇOKLU UYAP — ANA SAYFA KONSOLU (2026-11-11)
// ═══════════════════════════════════════════════════════════════════════════════════════
// UYAP ana sayfasındaki geniş banner (logo + ortada menü arama kutusu, ~330 px) yerine kısa bir
// banner ve dosya arama konsolu:
//   1. satır  kısayollar: İcra / Hukuk / Ceza birimleri (Dosya Sorgulama'daki kısayol çubuğuyla aynı liste)
//   2. satır  Yargı Türü · Yargı Birimi · Mahkeme (yazarak süzülür) · Açık/Kapalı
//   3. satır  "2026/1234" tek kutusu + Sorgula; sağda UYAP'ın menü arama kutusu (küçültülmüş)
// Sonuç (kullanıcı kararı): tek dosya → UYAP'ın kartı ana sayfada açılır; birden çok → Dosya
// Sorgulama sayfasına (yenilemeden) geçilir, form doldurulup UYAP'ın kendi sorgusu çalışır.
// Yargı birimi seçilmeden arama "Yakında" (veritabanından arama ileride, UyapMasa'daki gibi).
//
// UYAP'ın menü arama bileşeni (#searchId) KALDIRILMAZ: kartı açan kanca onun içinde (bkz.
// uyapKartiAc); yalnız küçültülüp sağ sütuna alınır. Banner arka planı (.dash-back, sabit konumlu
// dalga + logo) kısaltılır, logo gizlenir.
//
// UYAP uçları (canlı, 2026-11-11): search_phrase_detayli {dosyaDurumKod: 0 açık | 1 kapalı, pageSize,
// pageNumber, dosyaYil, dosyaSira, birimId: mahkeme ('' = o birimdeki tüm mahkemeler), birimTuru2:
// yargı birimi tablosu, birimTuru3: yargı türü}; avukat_mahkemeleri_sorgula {yargiTuru, yargiBirimi,
// dosyaKapaliMi} → [{birimAdi, birimId}]. Arama, enjekte betiğin önbelleğini (SWR) atlayan ham
// fetch'le yapılır (önbellek eski oturumun dosyaId'sini dönebiliyor).
(function() {
    'use strict';
    const anaCerceve = window.__uyapAnaCerceve ? window.__uyapAnaCerceve() : window.top === window;
    if (!anaCerceve || window.__uyapAnaKonsol) return;
    window.__uyapAnaKonsol = true;

    const MAHKEME_SAKLI = 'cokluuyapMahkemeler';
    const TURLER = [['', '— Yargı türü —'], ['2', 'İcra'], ['1', 'Hukuk'], ['0', 'Ceza']];
    const S = function() { return window.__uyapSorgu; };
    const kucuk = function(x) { return S() ? S().kucuk(x) : String(x || '').toLowerCase(); };

    let hamFn = null;
    function ham() {
        if (!hamFn) {
            try {
                const f = document.createElement('iframe');
                f.style.display = 'none';
                document.documentElement.appendChild(f);
                hamFn = f.contentWindow.fetch.bind(window);
            } catch (e) { hamFn = window.fetch.bind(window); }
        }
        return hamFn;
    }
    async function uyap(uc, govde) {
        const r = await ham()('/' + uc, { method: 'POST', cache: 'no-store', credentials: 'include',
            headers: { 'Content-Type': 'application/json', 'Accept': 'application/json, text/plain, */*' }, body: JSON.stringify(govde) });
        return r.json();
    }

    // ── Mahkeme listesi (yargı birimi + açık/kapalı başına; 7 gün saklanır) ──
    const mahkemeIstek = {};
    function mahkemeleriGetir(tur, birim, kapali) {
        const a = tur + '|' + birim + '|' + (kapali ? 1 : 0);
        try {
            const o = JSON.parse(localStorage.getItem(MAHKEME_SAKLI) || '{}') || {};
            if (o[a] && Date.now() - o[a].t < 7 * 86400000 && o[a].l.length) return Promise.resolve(o[a].l);
        } catch (e) {}
        if (mahkemeIstek[a]) return mahkemeIstek[a];
        return (mahkemeIstek[a] = uyap('avukat_mahkemeleri_sorgula.ajx', { yargiTuru: tur, yargiBirimi: birim, dosyaKapaliMi: !!kapali })
            .then(function(j) {
                const l = (Array.isArray(j) ? j : []).filter(function(m) { return m && m.birimId; })
                    .map(function(m) { return { ad: String(m.birimAdi || ''), id: String(m.birimId) }; });
                if (l.length) {
                    try {
                        const o = JSON.parse(localStorage.getItem(MAHKEME_SAKLI) || '{}') || {};
                        o[a] = { t: Date.now(), l: l };
                        localStorage.setItem(MAHKEME_SAKLI, JSON.stringify(o));
                    } catch (e) {}
                }
                return l;
            }).catch(function() { return []; }).then(function(l) { delete mahkemeIstek[a]; return l; }));
    }

    // ── Durum ──
    // Açılışta hiçbir şey seçili değil; son seçim hatırlanmaz (kullanıcı bulgusu 2026-11-11: dosya no yazınca
    // İcra Dairesi kendiliğinden seçili görünüyordu).
    const d = { tur: '', birim: '', mahkeme: null, kapali: false, birimler: [], mahkemeler: [] };
    try { localStorage.removeItem('cokluuyapAnaSecim'); } catch (e) {}     // eski sürümün "son seçim" kaydı

    // ── Stil ──
    function stilKur() {
        if (document.getElementById('cu-ana-stil')) return;
        const st = document.createElement('style');
        st.id = 'cu-ana-stil';
        st.textContent = [
            // Banner: dalga görseli ve logo kaldırılır; yerine üst çubukla aynı düz zemin, kısa.
            // .dash-back UYAP'ta ekrana SABİT (position:fixed; canlı 2026-11-11: 328 px, svg dalga +
            // img.dash-logo): sayfa kayınca konsol yukarı gidiyor, renkli zemin yerinde kalıyordu (kullanıcı
            // bulgusu). Zemin sayfa içeriğine bağlanır (absolute, içerik kapsayıcısının tepesinden), birlikte kayar.
            'body.cu-ana #page-content-wrapper{position:relative}',
            'body.cu-ana .dash-back{position:absolute!important;top:0!important;left:0!important;right:0!important;width:auto!important;margin:0!important;',
            'height:var(--cu-ana-yuk,168px)!important;overflow:hidden;background:var(--cu-ana-zemin,var(--cu-marka-alt))!important}',
            'body.cu-ana .dash-back svg,body.cu-ana .dash-back .dash-logo{display:none!important}',
            // UYAP 1200 px altında arama alanına kendi lacivertini (--bs-primary) veriyor, 992 px altında .dash-back'i
            // gizliyor (kullanıcı bulgusu 2026-11-11, telefon): zemin üst çubuğun rengi olsun; satırlar sığmazsa alta insin.
            '@media (max-width:1200px){body.cu-ana .search-area{background-color:var(--cu-ana-zemin,var(--cu-marka-alt))!important;background-image:none!important}}',
            // Telefon (< 992 px): UYAP'ın -30 px yan payı ve .container genişliği konsolu sola kaydırıyordu, sağda krem
            // şerit kalıyordu (kullanıcı bulgusu 2026-11-11). Pay sıfır; zemin ekranın iki kenarına taşar (gölge +
            // clip-path); gruplar ortalı, metin kutuları tam genişlik. Masaüstü (>= 992 px) etkilenmez.
            '@media (max-width:991px){'+
              'body.cu-ana .search-area{margin:0!important;max-width:none!important;width:auto!important;padding:10px 12px!important;'+
                'box-shadow:0 0 0 100vmax var(--cu-ana-zemin,var(--cu-marka-alt));clip-path:inset(0 -100vmax)}'+
              'body.cu-ana .search-area .row.cu-ana-satir{flex-wrap:wrap!important;margin-left:0!important;margin-right:0!important}'+
              'body.cu-ana .search-area .row.cu-ana-satir>*{padding-left:0!important;padding-right:0!important}'+
              'body.cu-ana .cu-ana-panel{padding:2px 0 4px;max-width:none}'+
              'body.cu-ana .cu-ana-grup{justify-content:center;margin:0 4px}'+
              'body.cu-ana .cu-ana-sec.tur{flex:1 1 90px}body.cu-ana .cu-ana-sec.birim{flex:2 1 170px;width:auto}'+
              'body.cu-ana .cu-ana-mah{flex:1 1 100%;max-width:none}body.cu-ana .cu-ana-no{flex:1 1 100%;width:100%}'+
            '}',
            'body.cu-ana .search-area .row.cu-ana-satir{flex-wrap:nowrap!important;align-items:flex-end}',
            'body.cu-ana .search-area .row.cu-ana-satir>.col-3{display:none!important}',
            // UYAP'ın menü arama kutusu (kullanıcı kararı 2026-11-11: gizli). DOM'dan SİLİNMEZ: görev çubuğunun
            // kartı açtığı kanca (#searchId bileşeni) onun içinde; display:none kancayı etkilemez.
            'body.cu-ana .cu-ana-menu{display:none!important}',
            'body.cu-ana .cu-ana-menu>:not(.search-back){display:none!important}',
            'body.cu-ana .cu-ana-menu #searchId{margin-bottom:0!important;padding:0!important}',
            'body.cu-ana .cu-ana-menu #searchId input{height:30px;font-size:13px}',
            'body.cu-ana .search-area{padding-top:2px}',
            'body.cu-ana .tour-2 + .row + .container.mt-5,body.cu-ana .container.mt-5{margin-top:14px!important}',
            // Alt bölümler (kullanıcı kararı 2026-11-11): yalnız Bildirimler ve Yaklaşan Duruşmalarım
            // kalır; Bildirimler ile Duruşmalar ekranı ikiye bölerek yan yana durur.
            'body.cu-ana .cu-ana-gizli{display:none!important}',
            'body.cu-ana .cu-ana-yarim{flex:0 0 50%!important;max-width:50%!important;width:50%!important}',
            '@media (max-width:991px){body.cu-ana .cu-ana-yarim{flex:0 0 100%!important;max-width:100%!important;width:100%!important}}',
            // Konsol
            // Simetrik, ağırlığı ortada: satırlar ortalanır, konsol en fazla 1080 px.
            '.cu-ana-panel{flex:1 1 auto;min-width:0;max-width:1080px;margin:0 auto;padding:4px 12px 6px;color:var(--cu-ust-yazi,#fff);font:13px/1.4 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif}',
            '.cu-ana-sat{display:flex;flex-wrap:wrap;align-items:center;justify-content:center;gap:6px;margin-top:8px}',
            '.cu-ana-sat:first-child{margin-top:0}',
            '.cu-ana-grup{display:flex;flex-wrap:wrap;align-items:center;gap:4px;margin:0 8px}',
            '.cu-ana-grup>b{font-size:12px;font-weight:600;color:var(--cu-ust-soluk,rgba(255,255,255,0.75));margin-right:2px}',
            '.cu-ana-cip{padding:2px 10px;border-radius:12px;border:1px solid var(--cu-ust-kenar,rgba(255,255,255,0.55));background:transparent;color:var(--cu-ust-yazi,#fff);font-size:12px;line-height:18px;cursor:pointer}',
            '.cu-ana-cip:hover{background:var(--cu-ust-hover,rgba(255,255,255,0.18))}',
            '.cu-ana-cip.sec{background:#fff;color:var(--cu-vurgu-yazi);border-color:#fff;font-weight:600}',
            '.cu-ana-sec,.cu-ana-girdi{height:32px;padding:4px 8px;border:1px solid rgba(255,255,255,0.35);border-radius:6px;background:#fff;color:#212529;font-size:13px;outline:none}',
            '.cu-ana-sec:focus,.cu-ana-girdi:focus{border-color:#86b7fe;box-shadow:0 0 0 2px rgba(134,183,254,0.45)}',
            '.cu-ana-sec.tur{width:96px}.cu-ana-sec.birim{width:210px}.cu-ana-sec.durum{width:92px}',
            '.cu-ana-mah{position:relative;flex:1 1 260px;min-width:200px;max-width:420px}',
            '.cu-ana-mah .cu-ana-girdi{width:100%}',
            '.cu-ana-liste{position:absolute;left:0;right:0;top:34px;z-index:1400;max-height:280px;overflow:auto;background:#fff;color:#212529;border:1px solid #ced4da;border-radius:6px;box-shadow:0 8px 24px rgba(0,0,0,0.25);display:none}',
            '.cu-ana-liste div{padding:5px 10px;cursor:pointer;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}',
            '.cu-ana-liste div:hover,.cu-ana-liste div.sec{background:var(--cu-vurgu-zemin)}',
            '.cu-ana-no{width:min(420px,100%);font-size:14px}',
            '.cu-ana-dugme{height:32px;padding:0 18px;border:0;border-radius:6px;background:#fff;color:var(--cu-vurgu-yazi);font-weight:700;cursor:pointer}',
            '.cu-ana-dugme:hover{background:var(--cu-vurgu-zemin)}.cu-ana-dugme:disabled{opacity:.6;cursor:default}',
            '.cu-ana-mesaj{flex-basis:100%;text-align:center;font-size:12px;color:#fde68a;min-height:16px;margin-top:2px}',
            '.cu-ana-mesaj.hata{color:#fecaca}.cu-ana-mesaj.yakinda{color:#bfdbfe}',
            // Bildirimler + Yaklaşan Duruşmalar: "ajanda" görünümü (kullanıcı kararı 2026-11-11); UYAP'ın kartları yerinde.
            'body.cu-ana :is(.notify-area,.tour-6) .hedef-card{border-radius:10px!important;border:1px solid var(--cu-t-kenar,rgba(127,127,127,.2))!important;box-shadow:0 1px 3px rgba(0,0,0,.06)!important;overflow:hidden}',
            'body.cu-ana :is(.notify-area,.tour-6) span.fs-6{display:inline-block;font-size:14px!important;font-weight:600;padding-left:9px;border-left:3px solid var(--cu-vurgu);line-height:1.2}',
            'body.cu-ana :is(.notify-area,.tour-6) .left-navigation{font-size:12px;font-weight:600;color:var(--cu-vurgu)!important;text-decoration:none}',
            'html[data-cu-tema="koyu"] body.cu-ana :is(.notify-area,.tour-6) span.fs-6{border-left-color:var(--cu-vurgu-parlak)}',
            'html[data-cu-tema="koyu"] body.cu-ana :is(.notify-area,.tour-6) .left-navigation{color:var(--cu-vurgu-parlak)!important}',
            'body.cu-ana :is(.notify-area,.tour-6) .empty-state-card{font-size:13px;opacity:.7}',
            'body.cu-ana :is(.notify-area,.tour-6) .empty-state-card i{font-size:28px!important}',
            'body.cu-ana .tour-6 ul.schedule-list{padding:4px 0!important;margin:0!important;height:auto!important}',
            'body.cu-ana .tour-6 ul.schedule-list>li.cu-ajanda{position:relative;display:grid!important;grid-template-columns:48px minmax(0,1fr) auto;grid-template-areas:"gun baslik baslik" "gun saat etiket";column-gap:12px;row-gap:3px;align-items:center;padding:8px 12px 8px 10px!important;margin:0!important;border-left:3px solid transparent;border-bottom:1px solid var(--cu-t-kenar,rgba(127,127,127,.15))}',
            'body.cu-ana .tour-6 ul.schedule-list>li.cu-ajanda:last-child{border-bottom:0}',
            'body.cu-ana .tour-6 ul.schedule-list>li.cu-ajanda::before,body.cu-ana .tour-6 ul.schedule-list>li.cu-ajanda::after{display:none!important}',
            'body.cu-ana .tour-6 ul.schedule-list>li.cu-ajanda>.dot-circle,body.cu-ana .tour-6 ul.schedule-list>li.cu-ajanda>.icon-list-item{display:none!important}',
            'body.cu-ana .tour-6 .cu-ajanda-gun{grid-area:gun;display:flex;flex-direction:column;align-items:center;justify-content:center;height:48px;border-radius:8px;background:var(--cu-vurgu-zemin);color:var(--cu-vurgu-yazi);line-height:1}',
            'body.cu-ana .tour-6 .cu-ajanda-gun b{font-size:20px;font-weight:700;font-variant-numeric:tabular-nums}',
            'body.cu-ana .tour-6 .cu-ajanda-gun i{font-style:normal;font-size:10px;font-weight:600;letter-spacing:.06em;margin-top:3px}',
            'body.cu-ana .tour-6 li.cu-ajanda>.icon-list-text{grid-area:baslik;margin:0!important;font-size:13px;font-weight:600;line-height:1.3;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden}',
            'body.cu-ana .tour-6 .cu-ajanda-saat{grid-area:saat;font-size:12px;opacity:.75;font-variant-numeric:tabular-nums;white-space:nowrap}',
            'body.cu-ana .tour-6 li.cu-ajanda>.icon-list-tag{grid-area:etiket;display:flex;flex-wrap:wrap;justify-content:flex-end;align-items:center;gap:4px;margin:0!important;font-size:0}',
            'body.cu-ana .tour-6 li.cu-ajanda>.icon-list-tag>*{font-size:11px!important;border-radius:10px!important;padding:1px 8px!important;line-height:16px!important;margin:0!important}',
            'body.cu-ana .tour-6 .cu-ajanda-goreli{font-weight:600;border:1px solid var(--cu-t-kenar,rgba(127,127,127,.35));opacity:.8}',
            'body.cu-ana .tour-6 .cu-ajanda-goreli.bugun{background:var(--cu-vurgu);color:var(--cu-vurgu-uzeri,#fff);border-color:var(--cu-vurgu);opacity:1}',
            'body.cu-ana .tour-6 .cu-ajanda-goreli.yarin{border-color:var(--cu-vurgu);color:var(--cu-vurgu-yazi);opacity:1}',
            'body.cu-ana .tour-6 .cu-ajanda-goreli.gecti{opacity:.55}',
            'body.cu-ana .tour-6 ul.schedule-list>li.cu-bugun{background:rgba(var(--cu-vurgu-rgb),.07);border-left-color:var(--cu-vurgu)}',
            'html[data-cu-tema="koyu"] body.cu-ana .tour-6 .cu-ajanda-gun{background:rgba(var(--cu-vurgu-parlak-rgb),.16);color:var(--cu-vurgu-parlak)}',
            'html[data-cu-tema="koyu"] body.cu-ana .tour-6 .cu-ajanda-goreli.yarin{color:var(--cu-vurgu-parlak);border-color:var(--cu-vurgu-parlak)}',
            'html[data-cu-tema="koyu"] body.cu-ana .tour-6 .cu-ajanda-goreli.bugun{background:var(--cu-vurgu-parlak);border-color:var(--cu-vurgu-parlak);color:#111}',
            'html[data-cu-tema="koyu"] body.cu-ana .tour-6 ul.schedule-list>li.cu-bugun{background:rgba(var(--cu-vurgu-parlak-rgb),.10);border-left-color:var(--cu-vurgu-parlak)}',
            'body.cu-ana .notify-area ul.notify-list{padding:4px 0!important;margin:0!important;border:0!important}',
            'body.cu-ana .notify-area li.notify-item{position:relative;display:grid!important;grid-template-columns:minmax(0,1fr) auto;grid-template-areas:"baslik tarih" "metin metin";column-gap:10px;row-gap:2px;padding:8px 12px 8px 11px!important;margin:0!important;border-left:3px solid var(--cu-vurgu);border-bottom:1px solid var(--cu-t-kenar,rgba(127,127,127,.15));transition:background-color .12s}',
            'body.cu-ana .notify-area li.notify-item:last-child{border-bottom:0}',
            'body.cu-ana .notify-area li.notify-item::before,body.cu-ana .notify-area li.notify-item::after{display:none!important}',
            'body.cu-ana .notify-area li.notify-item:hover{background:rgba(var(--cu-vurgu-rgb),.06)}',
            'html[data-cu-tema="koyu"] body.cu-ana .notify-area li.notify-item{border-left-color:var(--cu-vurgu-parlak)}',
            'body.cu-ana .notify-area li.notify-item>.icon-list--circle-icon{display:none!important}',
            'body.cu-ana .notify-area li.notify-item>.icon-list-baslik{grid-area:baslik;padding:0!important;font-size:13px;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}',
            'body.cu-ana .notify-area li.notify-item>.icon-list-baslik[data-cu-baslik]{font-size:0}',
            'body.cu-ana .notify-area li.notify-item>.icon-list-baslik[data-cu-baslik]::before{content:attr(data-cu-baslik);font-size:13px}',
            'body.cu-ana .notify-area li.notify-item>.icon-list-date{grid-area:tarih;font-size:11.5px;opacity:.65;white-space:nowrap;font-variant-numeric:tabular-nums}',
            'body.cu-ana .notify-area li.notify-item>.icon-list-text{grid-area:metin;margin:0!important;font-size:12.5px;line-height:1.35;opacity:.75;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden}',
        ].join('');
        document.head.appendChild(st);
    }

    // ── Konsolu kur ──
    let panel = null, el = {};
    function e(tag, sinif, metin) { const x = document.createElement(tag); if (sinif) x.className = sinif; if (metin != null) x.textContent = metin; return x; }
    function kur() {
        const ara = document.getElementById('searchId');
        const satir = document.querySelector('.search-area > .row');
        if (!ara || !satir) return false;
        if (satir.querySelector('.cu-ana-panel')) { document.body.classList.add('cu-ana'); return true; }
        stilKur();
        satir.classList.add('cu-ana-satir');
        // Konsol, alttaki içerikle (Duyurular / Bildirimler: Bootstrap .container) aynı genişlikte ve
        // ortalanmış dursun; sol menü kapalıyken ekranın soluna yapışıyordu.
        const alan_ = satir.closest('.search-area');
        if (alan_) alan_.classList.add('container');
        const menuSutun = ara.closest('.search-area > .row > div');
        if (menuSutun) menuSutun.classList.add('cu-ana-menu');
        panel = e('div', 'cu-ana-panel');
        // 1) Kısayollar
        const s1 = e('div', 'cu-ana-sat');
        el.cipler = [];
        (S() ? S().KISAYOLLAR : []).forEach(function(g) {
            const grup = e('div', 'cu-ana-grup');
            grup.appendChild(e('b', null, g[0] + ':'));
            g[1].forEach(function(k) {
                const b = e('button', 'cu-ana-cip', k[0]);
                b.type = 'button';
                b.title = g[0] + ' → ' + k[0] + ' (Mahkeme kutusuna geçilir; yazarak arayın)';
                b.addEventListener('click', function() { kisayol(k[1], k[2]); });
                b._k = k;
                el.cipler.push(b);
                grup.appendChild(b);
            });
            s1.appendChild(grup);
        });
        // 2) Seçimler
        const s2 = e('div', 'cu-ana-sat');
        el.tur = e('select', 'cu-ana-sec tur');
        el.tur.title = 'Yargı Türü';
        TURLER.forEach(function(t) { const o = e('option', null, t[1]); o.value = t[0]; el.tur.appendChild(o); });
        el.birim = e('select', 'cu-ana-sec birim');
        el.birim.title = 'Yargı Birimi';
        const mah = e('div', 'cu-ana-mah');
        el.mah = e('input', 'cu-ana-girdi');
        el.mah.type = 'text';
        el.mah.autocomplete = 'off';
        el.mah.placeholder = 'Mahkeme / daire (yazarak arayın; boş = hepsi)';
        el.liste = e('div', 'cu-ana-liste');
        mah.append(el.mah, el.liste);
        el.durum = e('select', 'cu-ana-sec durum');
        el.durum.title = 'Dosya durumu';
        [['0', 'Açık'], ['1', 'Kapalı']].forEach(function(t) { const o = e('option', null, t[1]); o.value = t[0]; el.durum.appendChild(o); });
        s2.append(el.tur, el.birim, mah, el.durum);
        // 3) Dosya no + Sorgula
        const s3 = e('div', 'cu-ana-sat');
        el.no = e('input', 'cu-ana-girdi cu-ana-no');
        el.no.type = 'text';
        el.no.autocomplete = 'off';
        el.no.placeholder = 'Dosya No: 2026/1234 — ya da "İzmir 15. İcra Dairesi 2026/1234" yapıştırın';
        el.sorgula = e('button', 'cu-ana-dugme', 'Sorgula');
        el.sorgula.type = 'button';
        el.mesaj = e('div', 'cu-ana-mesaj');
        s3.append(el.no, el.sorgula, el.mesaj);
        panel.append(s1, s2, s3);
        satir.insertBefore(panel, satir.firstChild);
        document.body.classList.add('cu-ana');
        bannerYuksekligi();
        olaylar();
        el.tur.value = d.tur;
        el.durum.value = d.kapali ? '1' : '0';
        birimleriDoldur().then(function() { mahkemeleriDoldur(); });
        return true;
    }

    // Banner zemini: üst çubuğun rengi; yüksekliği: konsolun altı + biraz boşluk (dar ekranda satırlar
    // alt alta kayınca banner da uzar).
    function degiskenYaz(ad, deger) {
        const h = document.documentElement;
        if (h.style.getPropertyValue(ad) !== deger) h.style.setProperty(ad, deger);
    }
    function bannerYuksekligi() {
        const nav = document.querySelector('.u-navbar');
        // Deri temalarında üst çubuk artık açık yüzeyli (tema.css, 2026-11-23); banner beyaz yazılı düğmeleriyle
        // temanın dolu rengini (--cu-t-ust) korur. UYAP / Koyu temalarında eskisi gibi üst çubuğun rengi.
        if (document.documentElement.hasAttribute('data-cu-deri')) degiskenYaz('--cu-ana-zemin', 'var(--cu-t-ust)');
        else if (nav) degiskenYaz('--cu-ana-zemin', getComputedStyle(nav).backgroundColor);
        if (!panel) return;
        const db = document.querySelector('.dash-back');
        const ust = db ? db.getBoundingClientRect().top : 60;
        const alt = panel.getBoundingClientRect().bottom;
        if (alt > ust) degiskenYaz('--cu-ana-yuk', Math.round(alt - ust + 16) + 'px');
    }
    window.addEventListener('resize', function() { if (panel) bannerYuksekligi(); });

    function mesaj(m, tur) { if (!el.mesaj) return; el.mesaj.textContent = m || ''; el.mesaj.className = 'cu-ana-mesaj' + (tur ? ' ' + tur : ''); }
    function ciplerCiz() {
        (el.cipler || []).forEach(function(b) {
            const k = b._k;
            const sec = k[1] === d.tur && d.birimler.some(function(x) { return x.tablo === d.birim && S() && S().birimBul([x], k[2]); });
            b.classList.toggle('sec', !!sec);
        });
    }
    async function birimleriDoldur(otoSec) {
        const l = d.tur && S() ? await S().birimListesiGetir(d.tur) : [];
        d.birimler = l || [];
        el.birim.disabled = !d.tur;
        el.birim.innerHTML = '';
        const bos = e('option', null, '— Yargı birimi —'); bos.value = ''; el.birim.appendChild(bos);
        d.birimler.forEach(function(b) { const o = e('option', null, b.kod); o.value = b.tablo; el.birim.appendChild(o); });
        if (!d.birimler.some(function(b) { return b.tablo === d.birim; })) d.birim = otoSec && d.birimler.length === 1 ? d.birimler[0].tablo : '';
        el.birim.value = d.birim;
        ciplerCiz();
    }
    async function mahkemeleriDoldur() {
        d.mahkemeler = [];
        if (!d.birim) { el.mah.value = ''; d.mahkeme = null; el.mah.disabled = true; return; }
        el.mah.disabled = false;
        const anahtar = d.tur + d.birim + d.kapali;
        const l = await mahkemeleriGetir(d.tur, d.birim, d.kapali);
        if (anahtar !== d.tur + d.birim + d.kapali) return;        // bu arada seçim değişti
        d.mahkemeler = l;
        const m = d.mahkeme && l.find(function(x) { return x.id === d.mahkeme.id; });
        d.mahkeme = m || null;
        el.mah.value = m ? m.ad : '';
    }

    // Mahkeme kutusu: Türkçe harf duyarsız, kelime kelime süzme ("izmir 15" → "İzmir 15. İcra Dairesi")
    let listeSecili = -1, listeOge = [], sessizOdak = false;
    // Kısayol / birim seçiminden sonra imleç doğrudan DOSYA NUMARASI kutusuna geçer (kullanıcı kararı 2026-11-11;
    // mahkeme isteğe bağlı, listesi kendiliğinden açılmaz). Dokunmatik ekranda odak verilmez (klavye açılmasın).
    const DOKUNMATIK = window.matchMedia && matchMedia('(pointer: coarse)').matches;
    function mahkemeyeGec() {
        if (DOKUNMATIK || !el.no) return;
        el.no.focus();
    }
    function listeCiz() {
        const q = kucuk(el.mah.value).split(' ').filter(Boolean);
        listeOge = d.mahkemeler.filter(function(m) { const a = kucuk(m.ad); return q.every(function(p) { return a.indexOf(p) >= 0; }); }).slice(0, 80);
        el.liste.innerHTML = '';
        listeOge.forEach(function(m, i) {
            const o = e('div', i === listeSecili ? 'sec' : null, m.ad);
            o.addEventListener('mousedown', function(ev) { ev.preventDefault(); mahkemeSec(m); });
            el.liste.appendChild(o);
        });
        el.liste.style.display = listeOge.length && document.activeElement === el.mah ? 'block' : 'none';
    }
    function mahkemeSec(m) {
        d.mahkeme = m;
        el.mah.value = m ? m.ad : '';
        el.liste.style.display = 'none';

        el.no.focus();
    }

    function olaylar() {
        el.tur.addEventListener('change', async function() { mesaj(''); d.tur = el.tur.value; d.birim = ''; d.mahkeme = null; await birimleriDoldur(true); mahkemeleriDoldur(); el.birim.focus(); });
        el.birim.addEventListener('change', function() { mesaj(''); d.birim = el.birim.value; d.mahkeme = null; ciplerCiz(); mahkemeleriDoldur(); mahkemeyeGec(); });
        el.durum.addEventListener('change', function() { d.kapali = el.durum.value === '1'; mahkemeleriDoldur(); });
        el.mah.addEventListener('focus', function() { listeSecili = -1; if (!sessizOdak) listeCiz(); });
        el.mah.addEventListener('mousedown', function() { if (document.activeElement === el.mah) { listeSecili = -1; listeCiz(); } });
        el.mah.addEventListener('blur', function() {
            el.liste.style.display = 'none';
            if (!el.mah.value.trim()) d.mahkeme = null;
            else if (!d.mahkeme || d.mahkeme.ad !== el.mah.value) {       // yazılan metin tek mahkemeye denk geliyorsa onu al
                listeCiz();
                if (listeOge.length === 1) mahkemeSec(listeOge[0]); else el.mah.value = d.mahkeme ? d.mahkeme.ad : '';
            }

        });
        el.mah.addEventListener('input', function() { listeSecili = 0; listeCiz(); });
        el.mah.addEventListener('keydown', function(ev) {
            if (ev.key === 'ArrowDown' || ev.key === 'ArrowUp') {
                ev.preventDefault();
                if (!listeOge.length) return;
                listeSecili = (listeSecili + (ev.key === 'ArrowDown' ? 1 : listeOge.length - 1)) % listeOge.length;
                listeCiz();
                const s = el.liste.children[listeSecili]; if (s) s.scrollIntoView({ block: 'nearest' });
            } else if (ev.key === 'Enter') {
                ev.preventDefault();
                if (listeOge[Math.max(0, listeSecili)]) mahkemeSec(listeOge[Math.max(0, listeSecili)]);
            } else if (ev.key === 'Escape') { el.liste.style.display = 'none'; }
        });
        el.no.addEventListener('input', function(ev) {
            if (ev.inputType === 'insertText' && /^\d{4}$/.test(el.no.value) && el.no.selectionStart === el.no.value.length) el.no.value += '/';
            mesaj('');
        });
        el.no.addEventListener('paste', function() { setTimeout(function() { metindenSec(el.no.value); }, 0); });
        el.no.addEventListener('keydown', function(ev) { if (ev.key === 'Enter') { ev.preventDefault(); sorgula(); } });
        // Sağ tık: Dosya Sorgulama'daki tek kutuyla aynı iki madde. Seçmeye gerek yok, kutunun tamamı
        // kopyalanır; Yapıştır kutudakini silip panodakini koyar, metinden mahkemeyi seçer ve sorgular.
        el.no.addEventListener('contextmenu', function(ev) {
            if (!window.__uyapSagMenuAc) return;
            ev.preventDefault();
            ev.stopPropagation();
            const bildir = window.__uyapBildirim || function() {};
            window.__uyapSagMenuAc(ev.clientX, ev.clientY, [[
                ['Dosya numarasını kopyala', function() {
                    const m = el.no.value.trim();
                    if (!m) { bildir('Kutu boş'); return; }
                    if (window.__uyapPanoya) window.__uyapPanoya(m);
                }],
                ['Yapıştır', async function() {
                    let m = '';
                    try { m = await navigator.clipboard.readText(); } catch (e) {
                        el.no.focus();
                        bildir('Pano okunamadı — Ctrl+V ile yapıştırın');
                        return;
                    }
                    el.no.value = String(m || '').replace(/\s+/g, ' ').trim();
                    el.no.focus();
                    await metindenSec(el.no.value);
                    sorgula();
                }],
            ]]);
        });
        el.sorgula.addEventListener('click', sorgula);
    }

    // ── Kısayol: tür + birim seçilir, mahkeme kutusuna geçilir ──
    async function kisayol(tur, desen) {
        mesaj('');
        d.tur = tur; el.tur.value = tur;
        await birimleriDoldur();
        const b = S() && S().birimBul(d.birimler, desen);
        d.birim = b ? b.tablo : '';
        el.birim.value = d.birim;
        d.mahkeme = null;

        ciplerCiz();
        await mahkemeleriDoldur();
        mahkemeyeGec();
    }

    // Yapıştırılan "İzmir 15. İcra Dairesi 2026/1234": birim türü ve mahkeme metinden bulunur.
    async function metindenSec(metin) {
        const k = kucuk(metin);
        if (!S()) return;
        let bulundu = null;
        S().KISAYOLLAR.forEach(function(g) { g[1].forEach(function(x) { if (!bulundu && k.indexOf(x[2]) >= 0) bulundu = x; }); });
        if (!bulundu) return;
        if (d.tur !== bulundu[1] || !d.birimler.some(function(x) { return x.tablo === d.birim && S().birimBul([x], bulundu[2]); })) {
            await kisayol(bulundu[1], bulundu[2]);
        }
        if (!d.mahkemeler.length) await mahkemeleriDoldur();
        const adaylar = d.mahkemeler.filter(function(m) { return k.indexOf(kucuk(m.ad)) >= 0; })
            .sort(function(a, b) { return b.ad.length - a.ad.length; });       // "İzmir 15." "İzmir 1."'den önce
        if (adaylar[0]) { d.mahkeme = adaylar[0]; el.mah.value = adaylar[0].ad; }
        const m = /(\d{4})\s*\/\s*(\d{1,8})(?!\d)/.exec(metin);
        if (m) el.no.value = m[1] + '/' + m[2];
        el.no.focus();
    }

    // ── Sorgula ──
    let sorguda = false;
    async function sorgula() {
        if (sorguda) return;
        const m = /(\d{4})\s*\/\s*(\d{1,8})(?!\d)/.exec(el.no.value);
        const yil = m ? Number(m[1]) : 0;
        if (!d.birim) { mesaj('Yakında: yargı birimi seçmeden, programın veritabanından dosya arama.', 'yakinda'); return; }
        if (!m || yil < 1900 || yil > new Date().getFullYear() + 1) { mesaj('Dosya numarasını 2026/1234 biçiminde yazın.', 'hata'); el.no.focus(); return; }
        sorguda = true;
        el.sorgula.disabled = true;
        mesaj('Sorgulanıyor…');
        try {
            const j = await uyap('search_phrase_detayli.ajx', { dosyaDurumKod: d.kapali ? 1 : 0, pageSize: 50, pageNumber: 1,
                dosyaYil: yil, dosyaSira: Number(m[2]), birimId: d.mahkeme ? d.mahkeme.id : '', birimTuru2: d.birim, birimTuru3: d.tur });
            const l = Array.isArray(j) && Array.isArray(j[0]) ? j[0] : (Array.isArray(j) ? j : []);
            const kayitlar = l.filter(function(x) { return x && x.dosyaId; });
            if (!Array.isArray(j)) {
                mesaj((j && (j.error || j.message)) || 'UYAP sorguyu yanıtlamadı.', 'hata');
            } else if (!kayitlar.length) {
                mesaj('Dosya bulunamadı' + (d.kapali ? ' (kapalı dosyalarda arandı).' : ' (açık dosyalarda arandı; kapalıysa durumu "Kapalı" yapın).'), 'hata');
            } else if (kayitlar.length === 1) {
                const k = kayitlar[0];
                const kart = { dosyaId: k.dosyaId, dosyaNo: k.dosyaNo, birimId: k.birimId, birimAdi: k.birimAdi, dosyaTurKod: k.dosyaTurKod,
                               dosyaTurKodAciklama: k.dosyaTur, dosyaDurum: k.dosyaDurum, dosyaDurumKod: k.dosyaDurumKod, birimTuru3: k.birimTuru3 };
                if (window.__uyapKartiAcDogrudan && window.__uyapKartiAcDogrudan(kart)) mesaj('');
                else mesaj('Dosya kartı açılamadı (UYAP sayfası değişmiş olabilir).', 'hata');
            } else {
                mesaj(kayitlar.length + ' dosya bulundu; Dosya Sorgulama listesine geçiliyor…');
                history.pushState({}, '', '/dosya-sorgulama');
                window.dispatchEvent(new PopStateEvent('popstate', { state: {} }));
                const hata = await S().formuDoldurVeSorgula({ tur: d.tur, birim: d.birim, mahkeme: d.mahkeme ? d.mahkeme.id : '',
                                                              kapali: d.kapali, yil: yil, sira: Number(m[2]) });
                if (hata && window.__uyapBildirim) window.__uyapBildirim(hata + ' Değerleri elle seçip Sorgula\'ya basın.');
            }
        } catch (err) {
            mesaj('Sorgu yapılamadı: ' + (err && err.message || err), 'hata');
        } finally {
            sorguda = false;
            if (el.sorgula) el.sorgula.disabled = false;
        }
    }

    // ── Alt bölümler: yalnız Bildirimler (notify-area) ve Yaklaşan Duruşmalarım (tour-6)
    // kalır; diğerleri (Basit/Detaylı/Yargıtay/Danıştay arama kutucukları, ödeme uyarısı, E-Duruşma / SMS
    // tanıtımı, Doküman Editörü / E-imza / Java kutusu, duruşma bilgisi tanıtımı) gizlenir. DOM'dan
    // kaldırılmaz (React'in kendi düğümleri); indirme kutusundaki bağlantılar Araçlar'da sunulur.
    const KALAN = /\b(notify-area|tour-6)\b/;     // ikisi yan yana, yarım genişlik; Duyurular (tour-3) da gizli
    let indirmeKutusu = null;
    function altTemizle() {
        const satir = document.querySelector('.container.mt-5 > .row');
        if (!satir) return;
        Array.prototype.forEach.call(satir.children, function(c) {
            const s = String(c.className);
            if (KALAN.test(s)) { c.classList.add('cu-ana-yarim'); return; }
            if (!c.classList.contains('cu-ana-gizli')) c.classList.add('cu-ana-gizli');
            if (/Doküman Editörü|JAVA Uygulaması/i.test(c.textContent || '')) indirmeKutusu = c;
        });
    }
    // ── Kartların "ajanda" görünümü (kullanıcı kararı 2026-11-11). UYAP'ın satırlarına (React'in li'leri) yalnız
    // küçük ek öğeler konur; React'in metinlerine dokunulmaz. Satır tarihi değişirse (React yeniden çizerse)
    // ekler silinip yeniden kurulur, böylece her turda güvenle çalışır.
    const AYLAR = ['OCA', 'ŞUB', 'MAR', 'NİS', 'MAY', 'HAZ', 'TEM', 'AĞU', 'EYL', 'EKİ', 'KAS', 'ARA'];
    function tarihAyristir(m) {
        m = String(m || '');
        let r = /(\d{1,2})[./-](\d{1,2})[./-](\d{4})(?:\D+(\d{1,2})[:.](\d{2}))?/.exec(m);
        if (r) return { g: +r[1], a: +r[2], y: +r[3], saat: r[4] != null ? ('0' + r[4]).slice(-2) + ':' + r[5] : '' };
        r = /(\d{4})-(\d{2})-(\d{2})(?:[T ](\d{2}):(\d{2}))?/.exec(m);
        if (r) return { g: +r[3], a: +r[2], y: +r[1], saat: r[4] != null ? r[4] + ':' + r[5] : '' };
        return null;
    }
    function goreliGun(t, simdi) {
        const bugun = new Date(simdi.getFullYear(), simdi.getMonth(), simdi.getDate());
        const fark = Math.round((new Date(t.y, t.a - 1, t.g) - bugun) / 864e5);
        if (fark === 0) return ['Bugün', 'bugun'];
        if (fark === 1) return ['Yarın', 'yarin'];
        if (fark > 1) return [fark + ' gün', ''];
        return ['Geçti', 'gecti'];
    }
    window.__cuAjanda = { tarihAyristir: tarihAyristir, goreliGun: goreliGun };   // deneme / hata ayıklama
    function kartlariSusle() {
        const simdi = new Date();
        document.querySelectorAll('.tour-6 ul.schedule-list > li').forEach(function(li) {
            const st = li.querySelector(':scope > .icon-list-item strong');
            const metin = st ? st.textContent.trim() : '';
            const anahtar = metin + '|' + simdi.toDateString();
            if (li.dataset.cuTarih === anahtar) return;
            li.querySelectorAll('.cu-ajanda-gun, .cu-ajanda-saat, .cu-ajanda-goreli').forEach(function(x) { x.remove(); });
            li.classList.remove('cu-ajanda', 'cu-bugun');
            li.dataset.cuTarih = anahtar;
            const t = tarihAyristir(metin);
            if (!t) return;                                   // tanınmayan biçim: UYAP'ın görünümü kalır
            const gun = e('span', 'cu-ajanda-gun');
            gun.appendChild(e('b', null, String(t.g)));
            gun.appendChild(e('i', null, AYLAR[t.a - 1] || ''));
            gun.title = metin;
            li.insertBefore(gun, li.firstChild);
            const etiket = li.querySelector(':scope > .icon-list-tag');
            li.insertBefore(e('span', 'cu-ajanda-saat', t.saat ? 'Saat ' + t.saat : ''), etiket || null);
            const g = goreliGun(t, simdi);
            const rozet = e('span', 'cu-ajanda-goreli' + (g[1] ? ' ' + g[1] : ''), g[0]);
            if (etiket) etiket.insertBefore(rozet, etiket.firstChild); else li.appendChild(rozet);
            li.classList.add('cu-ajanda');
            if (g[1] === 'bugun') li.classList.add('cu-bugun');
        });
        // Bildirim başlığının sonundaki " -" ayıracı: temiz başlık data özniteliğine yazılır, CSS onu gösterir.
        document.querySelectorAll('.notify-area li.notify-item > .icon-list-baslik').forEach(function(b) {
            const temiz = (b.textContent || '').replace(/\s*-\s*$/, '').trim();
            if (b.dataset.cuBaslik !== temiz) b.dataset.cuBaslik = temiz;
        });
    }

    // Çekmecenin Araçlar sekmesi için: UYAP'ın indirme bağlantıları (gizlenen kutudan, canlı 2026-11-11).
    // "JAVA Uygulaması" bağlantı değil, UYAP'ın kendi penceresini açan öğe: gizli kutudaki öğeye tıklanır;
    // ana sayfada değilsek önce ana sayfaya (yenilemeden) geçilir.
    const SABIT_INDIRMELER = [
        ['UYAP Doküman Editörü', 'https://www.uyap.gov.tr/UYAP-Editor'],
        ['E-imza Sağlayıcıları', 'https://www.btk.gov.tr/elektronik-sertifika-hizmet-saglayicilari'],
        ['Chrome', 'https://www.google.com/intl/tr_tr/chrome/'],
    ];
    window.__uyapIndirmeler = function() {
        const l = SABIT_INDIRMELER.map(function(x) {
            return { ad: x[0], ac: function() { window.open(x[1], '_blank', 'noopener'); } };
        });
        l.splice(2, 0, { ad: 'JAVA Uygulaması', ac: async function() {
            if (!document.getElementById('searchId')) {
                history.pushState({}, '', '/');
                window.dispatchEvent(new PopStateEvent('popstate', { state: {} }));
            }
            for (let i = 0; i < 40; i++) {
                altTemizle();
                const j = indirmeKutusu && Array.prototype.find.call(indirmeKutusu.querySelectorAll('div, a, span'), function(x) {
                    return x.children.length <= 2 && /^\s*JAVA Uygulaması\s*$/i.test(x.textContent || '');
                });
                if (j) { j.click(); return; }
                await new Promise(function(r) { setTimeout(r, 200); });
            }
            if (window.__uyapBildirim) window.__uyapBildirim('JAVA Uygulaması bağlantısı ana sayfada bulunamadı.');
        } });
        return l;
    };

    // UYAP ana sayfayı her girişte yeniden çizer: konsol yoksa yeniden kurulur; başka sayfada
    // banner düzeni (body.cu-ana) kaldırılır.
    function anaTur() {
        if (!document.body) return;
        const ana = !!document.getElementById('searchId') && !!document.querySelector('.search-area > .row');
        // Banner zemini her turda üst çubuğun güncel renginden: koyu temaya geçince lacivert kalmasın.
        if (ana) { if (!kur()) document.body.classList.remove('cu-ana'); else { altTemizle(); kartlariSusle(); bannerYuksekligi(); } }
        else document.body.classList.remove('cu-ana');
    }
    // DOM değişince bir sonraki çizimden önce (tema.js → __cuDegisince); yedek olarak saniyede bir.
    if (window.__cuDegisince) window.__cuDegisince(anaTur);
    setInterval(anaTur, 1000);
})();
