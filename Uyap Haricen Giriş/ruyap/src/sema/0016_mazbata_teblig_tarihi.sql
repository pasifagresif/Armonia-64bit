-- Elle bağlanan mazbatadaki tebliğ tarihi (23.11.2026). Eski PTT barkodları PTT'nin sisteminden silinir ("KAYIT YOK");
-- o tebligatın tebliğ günü yalnız mazbatanın üstünde yazar (fotoğraf, okunamaz). Kullanıcı bağlarken girer; kesinleşme
-- hesabında PTT'nin teslim günü yerine geçer (icra_durum::durumlari_guncelle). NULL: girilmedi, PTT bilgisi geçerli.
ALTER TABLE tebligat_mazbata_bagi ADD COLUMN IF NOT EXISTS teblig_tarihi date NULL;
