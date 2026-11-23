# Windows katmanı (yalnız ISO, `WINDOWS=1` — varsayılan)

Kullanıcı kararları (23.11.2026):
- Ağdaki **İcramatik** (paylaşılan klasörde, 32 bit) Armonia'da açılsın; masaüstü/dock kısayolu, tıklanınca ağ
  konumundaki klasörden açılır. Paylaşım **parolasız (misafir)**.
- Windows programı **ağa kapalı**; yalnız o klasöre erişsin.
- Debian'ın imzalı `wine` paketi + **bubblewrap kum havuzu** (Bottles değil: Bottles ilk açılışta internetten
  bileşen indiriyor, canlı ISO'da her açılışta yeniden kurulur). Bottles sonra, Office gibi kullanıcının kuracağı
  programlar için ayrıca değerlendirilecek.
- İcramatik 32 bit → Debian trixie Wine "eski WoW64": i386 kütüphaneleri gerekir → **ISO'da `ia32_emulation=0`
  KALDIRILIR** (kullanıcı onayı). `WINDOWS=0 iso_derle.sh` ile Windows katmanı olmayan, `ia32_emulation=0`'lı tam
  sertleştirilmiş ISO her zaman derlenebilir. Pi etkilenmez (arm64'te x86 Wine yok).

## Derlemeden önce
`includes.chroot/etc/armonia/windows.conf` dosyasında `PAYLASIM` (ör. `//192.168.1.10/Icramatik`) ve `PROGRAM`
(paylaşım köküne göre, ör. `Icramatik.exe`) doldurulur. Boşsa kısayol "Ayarlanmamış" bildirimi verir.

## Güvenlik çerçevesi
| Katman | Nasıl |
|---|---|
| Ağ yok | `bwrap --unshare-all` (yeni ağ ad alanı: yalnız kendi `lo`'su; dışarıya ve abstract X0 soketine ulaşım yok) |
| Ekran kafesi | Wine **ana masaüstünün Wayland soketini görmez**; iç içe `cage` içinde çalışır. Sanal klavye/işaretçi, pano okuma, ekran görüntüsü yalnız kafesin içinde kalır (denetçi B-1). Bedel: masaüstüyle kopyala-yapıştır yok. **Xwayland kapalı kalır** (O-X1) |
| Dosya | Kum havuzunda yalnız: `/usr` (salt okunur), Wine öneki, `~/Belgeler/Windows`, paylaşım klasörü, cage soketi. Ev dizininin geri kalanı, `/run/user`, D-Bus, `/dev/input`, pcscd, GPU **yok** |
| Yetki | `--cap-drop ALL`, `--disable-userns` (kum havuzunda yeni kullanıcı ad alanı yok), yeni oturum (TIOCSTI yok) |
| EXE bütünlüğü | `windows.conf` `SHA256` doluysa her açılışta doğrulanır, tutmazsa açılmaz; boşsa uyarı |
| Paylaşım | Root bağlar (`armonia-paylasim.service`), `username=guest`, `nosuid,nodev`, SMB1 yok (2.1+ pazarlık), dizin 0770 `armonia` |
| Bilinçli gevşeme 1 | `ia32_emulation=0` yok: 32 bit sistem çağrıları **tüm** süreçlere açık. Telafi: systemd servislerinde `SystemCallArchitectures=native` (tarayıcı birimi dahil), Wine kum havuzunda userns kapalı |
| Bilinçli gevşeme 2 | Masaüstü oturumuna **yazılabilir bir LAN klasörü** açılır: `oturum_yerel` (masaüstü yerel ağa kapalı) bu klasör için bilerek delinmiştir. Paylaşım parolasız → LAN'daki her makine okuyup yazabilir |

Z: sürücüsünün kaldırılması yalnız kolaylıktır, sınır değildir (Wine `\\?\unix\` yollarıyla görür); asıl sınır
bubblewrap'in bağlama ad alanıdır. Windows 11 24H2 sunucularda SMB imzalaması zorunlu olabilir ve misafir oturumu
reddedilebilir (DOĞRULANMADI) — paylaşımın bulunduğu Windows sürümü denemede görülecek.

Dosyalar: `hooks/0050-armonia-windows.hook.chroot`, `includes.chroot/` (bağlama servisi + betiği, başlatıcı,
`.desktop`, `windows.conf`). `iso_derle.sh` `WINDOWS=1` iken bunları kopyalar ve dock'a "İcramatik" ekler.
