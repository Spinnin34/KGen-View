# Auditoría técnica de KGen View

**Fecha:** 11 de noviembre de 2026
**Commit revisado:** `678dfb2` (`Harden archive and folder watch handling`)

## Alcance y conclusión

Se revisaron las 66 fuentes Rust y los archivos de compilación, CI, scripts, instalador y documentación. Se comprobaron también los recursos y artefactos en cuanto a su integración. No se modificó el código durante la auditoría.

No se encontraron fallos en las comprobaciones automatizadas ni una regresión de rendimiento repetible en las cargas medidas. Sí se identificaron dos defectos de prioridad media y tres problemas menores descritos abajo.

## Hallazgos

### Media — MCP puede descartar ediciones sin guardar

La herramienta `open_image` llama directamente a `start_load`. Las rutas de apertura de la interfaz pasan por `request_leave`, que protege los cambios pendientes; la llamada MCP omite esa comprobación. Si se abre otra imagen mientras el documento actual tiene ediciones sin guardar, una carga exitosa sustituye el documento y pierde esas ediciones.

**Ubicación:** [`src/app/controller/implementation/mcp.rs`](../src/app/controller/implementation/mcp.rs#L164)
**Corrección propuesta:** canalizar `open_image` por la misma transición protegida que usa la interfaz y conservar la semántica asíncrona de la respuesta MCP.

### Media — los cambios en los PNG de iconos no invalidan el build script

`build.rs` declara entradas `rerun-if-changed`, pero no declara `resources/filetype_pack`. Si solo cambia uno de esos PNG, Cargo puede reutilizar los `.ico` generados anteriormente y el instalador incluir iconos obsoletos.

**Ubicación:** [`build.rs`](../build.rs#L4)
**Corrección propuesta:** declarar el directorio `resources/filetype_pack` como entrada del build script.

### Menor — el texto del menú no actualiza su escala al cambiar de monitor

El formato de DirectWrite se crea una vez usando la escala DPI actual. El manejador de `WM_DPICHANGED` actualiza la ventana, pero no invalida ese formato; al mover la ventana entre monitores, el tamaño del texto del menú puede quedar desfasado respecto al resto de la interfaz.

**Ubicación:** [`src/ui/menu_bar.rs`](../src/ui/menu_bar.rs#L94), [`src/app/controller/implementation/window/wndproc.rs`](../src/app/controller/implementation/window/wndproc.rs#L69)
**Corrección propuesta:** reconstruir el formato de texto cuando cambie el DPI.

### Menor — las etiquetas de la barra de herramientas no se traducen

Las etiquetas al pasar el cursor están definidas en español y `label_for_hover` ignora el idioma activo. En la interfaz inglesa, la barra de estado sigue mostrando etiquetas en español.

**Ubicación:** [`src/ui/toolbar.rs`](../src/ui/toolbar.rs#L20), [`src/ui/mod.rs`](../src/ui/mod.rs#L159)
**Corrección propuesta:** resolver estas etiquetas mediante la tabla de traducciones según el idioma activo.

### Menor, auxiliar — el script incluido para IrfanView busca una carpeta que no existe

`run.bat` busca `Icons`, pero el directorio incluido se llama `Icon`. El script auxiliar no encuentra sus iconos con la estructura actual. No forma parte del flujo normal de KGen ni del instalador de KGen View.

**Ubicación:** [`vendor/IrfanView-Filetype-IconsChanger/run.bat`](../vendor/IrfanView-Filetype-IconsChanger/run.bat#L14), [`build.rs`](../build.rs#L29)
**Corrección propuesta:** hacer coincidir el nombre esperado con el directorio incluido.

## Validación

- `cargo fmt -- --check`: correcto.
- `cargo clippy --locked --all-targets -- -D warnings -A clippy::too_many_arguments`: correcto.
- `cargo test --locked`: 36 pruebas correctas, 0 fallidas.
- `cargo build --locked --release`: correcto.
- `git diff --check`: correcto.
- Las pruebas existentes cubren decodificación, transformaciones, edición y exportación, reservas concurrentes, navegación rápida, ciclo de vida de temporales ZIP y notificaciones de vigilancia.

## Rendimiento medido

Se usaron 15 procesos nuevos por modo para las imágenes grandes y cinco muestras para los catálogos. Los resultados de referencia proceden de `artifacts/performance-2026-11-11.json`; no se vació la caché del sistema operativo. Las medianas de decodificación en milisegundos fueron:

| Carga | Modo | Referencia | Auditoría |
|---|---|---:|---:|
| JPEG 24 MP | Original | 191,0 | 171,8 |
| PNG 24 MP | Original | 305,7 | 305,8 |
| JPEG 60 MP | Original | 435,0 | 424,5 |
| PNG 60 MP | Original | 738,8 | 735,5 |
| JPEG 24 MP | Vista previa | 177,7 | 166,5 |
| PNG 24 MP | Vista previa | 310,3 | 306,7 |
| JPEG 60 MP | Vista previa | 372,0 | 357,7 |
| PNG 60 MP | Vista previa | 760,9 | 718,4 |

El pico de memoria residente se mantuvo en rangos equivalentes a la referencia. El catálogo de 10.000 archivos dio una mediana de 11,7 ms; el de 100.000, 128,6 ms y p95 de 166,0 ms. La diferencia respecto a la referencia del catálogo de 100.000 (121,1 ms, una muestra) no es suficiente para confirmar una regresión.

## Límites

- La prueba visual de la ventana se interrumpió con Escape físico y no se reintentó; no se verificaron visualmente el cambio de DPI ni las etiquetas de la barra.
- No se midió la aplicación completa en un equipo con 4 GB ni se realizó una prueba visual prolongada de 1.000 navegaciones.
- Las mediciones cubren decodificación y catálogo, no latencia total de interfaz, GPU ni entrada del usuario.
