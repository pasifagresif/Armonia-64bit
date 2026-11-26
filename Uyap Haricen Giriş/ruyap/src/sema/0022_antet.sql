-- Büro antetleri (Ayarlar → Antetlerim; Armonia Yazı "Antet" menüsü). Resim UYAP'ın üst bilgide kullandığı
-- biçimde saklanır: PNG, base64; genişlik/yükseklik pt (UYAP antetlerinde 1 piksel = 1 pt). 26.11.2026
CREATE TABLE IF NOT EXISTS antet (
    id          bigserial PRIMARY KEY,
    ad          text NOT NULL DEFAULT '',
    resim       text NOT NULL,
    genislik    double precision NOT NULL,
    yukseklik   double precision NOT NULL,
    varsayilan  boolean NOT NULL DEFAULT false,
    eklenme     timestamptz NOT NULL DEFAULT now()
);
-- En çok bir varsayılan antet (yeni belgeler onunla açılır); hiç olmayabilir.
CREATE UNIQUE INDEX IF NOT EXISTS antet_tek_varsayilan ON antet (varsayilan) WHERE varsayilan;
