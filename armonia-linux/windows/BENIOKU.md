# Windows katmanı (yalnız ISO, `WINDOWS=1` — varsayılan)

**ÇOKLU UYGULAMA (2026-11-25):** Bu katman tek bir programa ("İcramatik") özel DEĞİLDİR; ağdaki paylaşılan
klasörden çalışan **herhangi sayıda** Windows programı eklenebilir. "İcramatik" sadece kullanıcının ilk denediği
örnektir, mimariye gömülü bir ad/simge değildir — bkz.
`docs/claude-notlar/hafiza/dosya-yoneticisi-ve-windows-genellestirme-plani.md`.

Kullanıcı kararları (23.11.2026, çoklu uygulamaya genelleştirildi 25.11.2026):
- Ağdaki Windows programları (paylaşılan klasörde, 32/64 bit) Armonia'da açılsın; her biri kendi masaüstü/dock
  kısayoluyla, tıklanınca kendi ağ konumundaki klasörden açılır. Paylaşım **parolasız (misafir)**.
- Her Windows programı **ağa kapalı**; yalnız kendi klasörüne erişir.
- Debian'ın imzalı `wine` paketi + **bubblewrap kum havuzu** (Bottles değil: Bottles ilk açılışta internetten
  bileşen indiriyor, canlı ISO'da her açılışta yeniden kurulur). Bottles sonra, Office gibi kullanıcının kuracağı
  programlar için ayrıca değerlendirilecek.
- 32 bit Windows programları → Debian trixie Wine "eski WoW64": i386 kütüphaneleri gerekir → **ISO'da
  `ia32_emulation=0` KALDIRILIR** (kullanıcı onayı, katmanda herhangi bir uygulama varken). `WINDOWS=0
  iso_derle.sh` ile Windows katmanı olmayan, `ia32_emulation=0`'lı tam sertleştirilmiş ISO her zaman derlenebilir.
  Pi etkilenmez (arm64'te x86 Wine yok).

## Derlemeden önce
Her uygulama için `includes.chroot/etc/armonia/windows.apps.d/_ornek.conf.ornek` dosyası, adı `<kimlik>.conf`
olacak şekilde kopyalanır (ör. `icramatik.conf`) ve içindeki `PAYLASIM` (ör. `//192.168.1.10/Icramatik`),
`PROGRAM` (paylaşım köküne göre, ör. `Icramatik.exe`) ve `AD` (dock'ta görünen ad) doldurulur. `<kimlik>` yalnız
küçük harf/rakam/`-`/`_` içerebilir (systemd birim adı ve dock kimliği olarak kullanılır). Boş `PAYLASIM`
bırakılırsa o uygulamanın kısayolu "Ayarlanmamış" bildirimi verir. `windows_hazirla.sh` (derleme script'i,
`iso_derle.sh` tarafından çağrılır) her `.conf` dosyası için otomatik `.desktop` + dock kutucuğu üretir;
`_ornek.conf.ornek` (uzantısı `.conf` DEĞİL) atlanır.

Armonia Ayarlar'daki "Windows programları" bölümünden de eklenip düzenlenebilir (root yardımcısı
`pencere-yardimci`, `pkexec` ile — bkz. `armonia-uygulamalar/windows_ayarlari`).

## Güvenlik çerçevesi
| Katman | Nasıl |
|---|---|
| Ağ yok | `bwrap --unshare-all` (yeni ağ ad alanı: yalnız kendi `lo`'su; dışarıya ve abstract X0 soketine ulaşım yok) |
| Ekran kafesi | Wine **ana masaüstünün Wayland soketini görmez**; iç içe `cage` içinde çalışır. Sanal klavye/işaretçi, pano okuma, ekran görüntüsü yalnız kafesin içinde kalır (denetçi B-1). Bedel: masaüstüyle kopyala-yapıştır yok. **Xwayland kapalı kalır** (O-X1) |
| Dosya | Kum havuzunda yalnız: `/usr` (salt okunur), Wine öneki, `~/Belgeler/Windows`, paylaşım klasörü, cage soketi. Ev dizininin geri kalanı, `/run/user`, D-Bus, `/dev/input`, pcscd, GPU **yok** |
| Yetki | `--cap-drop ALL`, `--disable-userns` (kum havuzunda yeni kullanıcı ad alanı yok), yeni oturum (TIOCSTI yok) |
| EXE bütünlüğü | `windows.apps.d/<kimlik>.conf` `SHA256` (ya da `<kimlik>.sha256` listesi) doluysa her açılışta doğrulanır, tutmazsa açılmaz; boşsa uyarı |
| Paylaşım | Root bağlar (`armonia-paylasim@<kimlik>.service`, her uygulama kendi örneği), `username=guest`, `nosuid,nodev`, SMB1 yok (2.1+ pazarlık), dizin 0770 `armonia` |
| Bilinçli gevşeme 1 | `ia32_emulation=0` yok: 32 bit sistem çağrıları **tüm** süreçlere açık. Telafi: systemd servislerinde `SystemCallArchitectures=native` (tarayıcı birimi dahil), Wine kum havuzunda userns kapalı |
| Bilinçli gevşeme 2 | Masaüstü oturumuna **yazılabilir bir LAN klasörü** açılır: `oturum_yerel` (masaüstü yerel ağa kapalı) bu klasör için bilerek delinmiştir. Paylaşım parolasız → LAN'daki her makine okuyup yazabilir |

Z: sürücüsünün kaldırılması yalnız kolaylıktır, sınır değildir (Wine `\\?\unix\` yollarıyla görür); asıl sınır
bubblewrap'in bağlama ad alanıdır. Windows 11 24H2 sunucularda SMB imzalaması zorunlu olabilir ve misafir oturumu
reddedilebilir (DOĞRULANMADI) — paylaşımın bulunduğu Windows sürümü denemede görülecek.

Dosyalar: `hooks/0050-armonia-windows.hook.chroot` (her tanımlı uygulama için `armonia-paylasim@<kimlik>` etkinleştirir),
`includes.chroot/` (template bağlama servisi + betiği, başlatıcı, `windows.apps.d/_ornek.conf.ornek`).
`windows_hazirla.sh` (repo köküne, `iso_derle.sh` `WINDOWS=1` iken çağırır) her `.conf` tanımı için `.desktop` +
dock kutucuğunu DERLEME ZAMANINDA üretir; hiçbir uygulama adı/simgesi koda gömülü değildir.
