-- Elle bağlanan mazbatanın sonucu (26.11.2026, kullanıcı bulgusu): iade dönen tebligatın tebliğ tarihi olmaz. iade = true:
-- mazbata "tebliğ edilemedi" döndü; tebligat İade sayılır, kesinleşme başlamaz (icra_durum::durumlari_guncelle).
-- teblig_tarihi o zaman isteğe bağlı iade günüdür. false: eskisi gibi (teblig_tarihi = mazbatadaki tebliğ günü ya da NULL).
ALTER TABLE tebligat_mazbata_bagi ADD COLUMN IF NOT EXISTS iade boolean NOT NULL DEFAULT false;
