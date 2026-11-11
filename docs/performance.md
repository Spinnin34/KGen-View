# Rendimiento y arquitectura

## Objetivo

Visor generalista para equipos de 4–8 GB, con fotografía de hasta 60 MP como carga habitual. La presentación es progresiva; la copia al portapapeles y la exportación conservan los píxeles originales y las operaciones aplicadas.

## Responsabilidades

- `imaging`: identidad y revisión del documento, fuente, metadatos y operaciones geométricas. Decodificación WIC, GIF, SVG, JPEG XL y codificación se separan en módulos.
- `app/controller`: coordinación de ventanas, navegación y servicios. La interfaz es propietaria de los recursos Direct2D; los trabajadores inicializan su propio apartamento COM.
- `resources`: reservas CPU/GPU compartidas y buffers de píxeles inmutables con copia al modificar. Clonar un documento o una instantánea no duplica sus píxeles.
- `mailbox`: un resultado pendiente por servicio; reemplazo y cierre liberan los objetos. Los mensajes Win32 notifican disponibilidad y no transportan punteros propietarios.

La navegación conserva únicamente la petición pendiente más reciente. Las escrituras explícitas usan un servicio que rechaza operaciones solapadas; conversión y guardado comparten además un cerrojo de exportación. El guardado se aplica a la ventana solo si coinciden documento y revisión. La navegación o el cierre solicitados durante un guardado continúan después de completarlo correctamente.

Las transformaciones geométricas se componen en una correspondencia entre coordenadas de pantalla, imagen editada y fuente original. WIC recibe el recorte y la orientación resultantes; las fotografías compatibles se exportan desde esa cadena sin crear primero una copia CPU completa. JPEG convierte la transparencia sobre blanco en bloques de filas.

## Presupuestos y límites

| Recurso | Límite por proceso |
|---|---:|
| Reservas CPU controladas, incluidas tareas activas | 512 MiB |
| Texturas de imagen Direct2D | 64 MiB |
| Vista previa o región solicitada | 32 MiB |
| Precarga de imágenes adyacentes | 16 MiB y 8 entradas |
| Caché de fotogramas compuestos GIF | 32 MiB |
| Trabajadores internos JPEG XL | 2 |

El historial guarda operaciones, no instantáneas de fotografías. Los buffers compartidos se contabilizan una sola vez y las reservas se liberan con su último propietario. Los presupuestos miden reservas de datos controlados; las asignaciones internas de WIC, resvg, Direct2D y los controladores se observan aparte en las métricas del proceso. JPEG XL reserva un límite conservador para su rastreador de asignaciones.

La cancelación es cooperativa. Algunos códecs solo permiten recuperar el control después de terminar una llamada nativa; la interfaz sigue disponible mientras tanto. GIF conserva un decodificador secuencial y fotogramas recientes; retroceder más allá de su caché requiere recomponer desde el principio. Las dimensiones, número de páginas y límites de entrada mantienen sus comprobaciones de seguridad.

## Reproducir mediciones

```powershell
cargo build --locked --release
.\scripts\new-benchmark-fixtures.ps1
.\scripts\benchmark.ps1 -Iterations 30
.\scripts\benchmark.ps1 -Iterations 30 -Original -OutputPath target\performance-original.json
```

El generador utiliza componentes incluidos en Windows. `-LargePng` añade PNG de 24 y 60 MP; `-LargeFolders` añade carpetas de 10.000 y 100.000 imágenes mediante grupos de enlaces físicos en un volumen NTFS. Los archivos existentes se conservan.

Las sondas también pueden ejecutarse individualmente:

```powershell
.\target\release\kgen-view.exe --benchmark-image target\performance-fixtures\photo-60mp.jpg | Out-String
.\target\release\kgen-view.exe --benchmark-image target\performance-fixtures\photo-60mp.jpg --original | Out-String
.\target\release\kgen-view.exe --benchmark-catalog target\performance-fixtures\folder-100000\image000000.png | Out-String
```

Cada muestra abre un proceso nuevo y registra duración, resolución presentada/original, bytes de píxeles, memoria privada y máximo de memoria residente. La caché del sistema operativo no se vacía. Estas mediciones cubren la decodificación o catálogo; no incluyen creación de ventana, GPU ni latencia de entrada. Comparar siempre el mismo archivo, perfil de compilación y equipo. La vista previa y el original hacen trabajos distintos; el informe distingue su calidad.

Para observar la aplicación completa desde una consola con captura de salida:

```powershell
$env:KGEN_PERF = '1'
.\target\release\kgen-view.exe C:\imagenes\foto.jpg 2> target\viewer-perf.log
```

El diagnóstico es local y optativo. Registra etapas y consumo reservado, sin rutas ni metadatos de las imágenes. `first_visible` se registra tras completar el primer dibujo de la nueva imagen; también se registran decodificación, reducción nativa, copia de píxeles, subida a GPU y exportación.

## Calidad y aceptación

```powershell
cargo fmt -- --check
cargo clippy --locked --all-targets -- -D warnings -A clippy::too_many_arguments
cargo test --locked
cargo build --locked --release
```

La integración continua de Windows usa Rust 1.99.0 y ejecuta esas comprobaciones. Las pruebas incluyen orientación EXIF, color y transparencia, exportación nativa desde vistas previas, TIFF bajo demanda, GIF, JPEG XL con alfa y regiones, fuentes modificadas externamente, reservas concurrentes, catálogos de 100.000 entradas, vida útil de ZIP y cancelación de vigilancia.

Objetivos para un equipo de referencia con 4 GB, SSD y pantalla 1080p: respuesta de interfaz p95 inferior a 50 ms; navegación desde caché inferior a 100 ms; CPU media en reposo inferior al 0,5 %; ausencia de crecimiento sostenido de memoria o identificadores tras 1.000 navegaciones. La validación en ese hardware y la latencia de entrada requieren una prueba de la aplicación completa; las sondas de decodificación por sí solas no acreditan esos objetivos.

## Resultado local del 11 de noviembre de 2026

Comparación dentro de la misma compilación optimizada, con cinco procesos nuevos por imagen y modo. Las imágenes grandes se presentan como una vista de hasta 1920 × 1080; la decodificación completa conserva todos sus píxeles.

| Imagen | Completa: mediana / pico residente | Vista previa: mediana / pico residente |
|---|---:|---:|
| photo-24mp.jpg | 191 ms / 106 MiB | 178 ms / 60 MiB |
| photo-24mp.png | 306 ms / 106 MiB | 310 ms / 21 MiB |
| photo-60mp.jpg | 435 ms / 244 MiB | 372 ms / 43 MiB |
| photo-60mp.png | 739 ms / 243 MiB | 761 ms / 22 MiB |

El JPEG de 60 MP reduce el máximo residente del decodificador aproximadamente un 82 %. PNG mantiene un tiempo de descompresión similar, con un máximo residente menor. El catálogo de 100.000 imágenes tardó 121 ms en una muestra local; el de 10.000 tardó 17 ms. La batería de 36 pruebas, formato, Clippy y compilación optimizada pasó. La apertura de imágenes también se observó en la ventana nativa.

Estos resultados no acreditan la latencia total de la interfaz ni el comportamiento en un equipo de 4 GB. Los [datos y muestras completos](../artifacts/performance-2026-11-11.json) permiten revisar las cifras.
