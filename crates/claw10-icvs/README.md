# claw10-icvs

Wrapper kompiler **InstructCanvas (ICVS)** untuk **Claw10 OS**.

Crate ini menyediakan antarmuka pengompilasian untuk format instruksi berbasis DAG (Directed Acyclic Graph) InstructCanvas agar kompatibel dengan runtime eksekusi agen.

## Fitur Utama
* **Parsing ICVS**: Membaca berkas skema instruksi InstructCanvas (.icvs).
* **Kompilasi Skema**: Mengubah sintaks alur kerja graf (.icvs) menjadi struktur data instruksi runtime yang dapat dieksekusi dengan cepat oleh agen.

## Cara Penggunaan
```toml
[dependencies]
claw10-icvs = { workspace = true }
```
