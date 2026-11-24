# KGen View

<p align="center">
  <img src="resources/kgen-view-logo.png" alt="KGen View" width="96">
</p>

<p align="center">
  Visor nativo de imágenes y vídeo para Windows.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/plataforma-Windows%2010%2F11%20x64-0078D4?style=flat-square" alt="Windows 10/11 x64">
  <img src="https://img.shields.io/badge/escrito%20en-Rust-DEA584?style=flat-square&logo=rust&logoColor=white" alt="Rust">
  <img src="https://img.shields.io/badge/licencia-MIT-2ea44f?style=flat-square" alt="Licencia MIT">
</p>

<p align="center">
  <img src="artifacts/KGenView-1.2-toolbar.png" alt="KGen View mostrando una imagen en modo oscuro" width="720">
</p>

KGen View es un visor de imágenes para Windows 10 y 11 de 64 bits. Está construido como una aplicación Win32 nativa en Rust, con decodificación mediante Windows Imaging Component (WIC) y renderizado acelerado con Direct2D. El resultado es una herramienta pequeña, directa y cómoda para abrir imágenes, recorrer carpetas y hacer ajustes rápidos sin convertir el visor en un editor pesado.

## Características

- Interfaz nativa, compacta y adaptada a pantallas con escalado DPI.
- Modo oscuro, modo claro y seguimiento del tema del sistema.
- Cambio de idioma entre español e inglés.
- Arrastrar y soltar, apertura desde el Explorador y apertura por línea de comandos.
- Navegación circular por las imágenes en el orden de la vista de la carpeta en el Explorador de archivos. Si la vista no está disponible, las imágenes se ordenan por nombre de forma natural (`imagen2.png` antes de `imagen10.png`). Los vídeos usan su propia lista por nombre.
- Reproducción de vídeos locales con sonido y controles flotantes, bucle, búsqueda por arrastre, velocidad y pantalla completa.
- Apertura de archivos ZIP y CBZ: indexado rápido y extracción bajo demanda mientras se navega por las imágenes.
- Al abrir desde el Explorador una imagen dentro de un ZIP, carga el archivo completo y se sitúa en esa imagen.
- Redimensionado automático de la ventana para aprovechar el espacio disponible.
- Zoom con rueda, zoom libre, tamaño real, ajuste a ventana y ajuste al ancho.
- Paneo y pantalla completa.
- GIF animado y navegación por fotogramas o páginas en documentos multipágina compatibles.
- Rotación de 90°, volteo horizontal y vertical, recorte de selección y deshacer/rehacer.
- Conversión desde **Editar → Convertir formato** a los formatos que Windows tenga disponibles para exportar.
- Panel de conversión flotante, con tema a juego y estado animado durante la exportación.
- Varias instancias independientes para abrir imágenes en ventanas separadas, también desde el Explorador.
- Opción **Ver → Siempre encima** para mantener la ventana activa sobre las demás aplicaciones.
- Guardar como con los codificadores de imagen detectados en Windows Imaging Component (WIC).
- Copiar la imagen o su ruta al portapapeles.
- Actualizaciones desde GitHub: la instalación comprueba versiones al iniciar y muestra un botón para descargar e instalar la publicación más reciente.
- Captura rectangular opcional con atajo global; copia la imagen y la guarda en `Imagenes\KGen` con autor de Windows, fecha UTC, titulo, descripcion y datos de KGen View en el PNG.
- Envío a la Papelera de reciclaje con confirmación configurable.
- Presentación con intervalo configurable.
- Salto automático de archivos dañados o incompatibles durante la navegación.
- Sin telemetría ni WebView; el proceso residente de captura solo se inicia al activar esa función.

## Formatos

### Apertura

KGen View abre directamente los siguientes formatos:

| Formato | Extensiones |
|---|---|
| Bitmap | `.bmp`, `.dib` |
| JPEG | `.jpg`, `.jpeg`, `.jpe` |
| PNG | `.png` |
| GIF | `.gif` |
| TIFF | `.tif`, `.tiff` |
| Iconos | `.ico` |
| WebP | `.webp` |
| Truevision TGA | `.tga` |
| Portable Anymap | `.pbm`, `.pam`, `.pgm`, `.ppm`, `.pnm` |
| QOI | `.qoi` |
| Radiance HDR | `.hdr` |
| OpenEXR | `.exr` |
| Farbfeld | `.ff` |
| AVIF | `.avif`, si hay un códec WIC compatible en Windows |
| HEIF / HEIC | `.heif`, `.heic` |
| JPEG XL | `.jxl` |
| SVG | `.svg` |
| Otros codecs WIC | `.wdp`, `.jxr`, `.hdp`, `.dds`, si el codec está disponible en Windows |
| Archivos de imágenes | `.zip`, `.cbz` (imágenes compatibles dentro del archivo) |

TGA, PBM/PAM/PGM/PPM/PNM, QOI, Radiance HDR, OpenEXR, Farbfeld y WebP tienen lectura y escritura integradas en KGen View. HDR y OpenEXR se adaptan al espacio de visualización de 8 bits de la aplicación; al guardarlos desde KGen no se conserva toda la profundidad ni el rango HDR de origen. AVIF, HEIF/HEIC, JPEG XR y DDS dependen de los códecs WIC disponibles en Windows. JPEG XL se decodifica dentro de KGen View, sin necesitar un códec adicional del sistema.

### Vídeo

El instalador y el ZIP portátil incluyen LibVLC 3 para reproducir archivos locales como MP4, MKV, WebM, MOV, AVI, WMV, MPEG, M2TS, FLV, OGV y 3GP. La compatibilidad depende del códec de las pistas y del contenedor. LibVLC usa decodificación por hardware cuando el códec, la GPU y el controlador son compatibles, y puede decodificar por software en los demás casos. La carpeta `vlc` incluye los avisos de licencia y enlaces al código fuente de VideoLAN.

Acerca el ratón al borde inferior para ver los controles flotantes. Al alejarlo o dejarlo quieto queda una línea naranja con la posición del vídeo. La línea de tiempo muestra la posición con milisegundos junto al puntero. Haz clic o arrastra para buscar, o selecciona una marca para saltar a un capítulo. El lateral derecho muestra el volumen vertical de 0 a 200 %, con una marca en el 100 %. Usa la rueda en ese lateral para ajustar el volumen. Los botones **Más opciones**, **Velocidad** y **Pistas y capítulos** abren un panel del reproductor. Desde ahí puedes ajustar la velocidad, elegir el audio o los subtítulos, ir a un capítulo, avanzar un fotograma o volver al punto anterior tras un salto. Usa la rueda o las flechas para recorrer la lista y Escape para cerrarla. El ajuste automático adapta la ventana al tamaño y orientación del vídeo dentro del monitor. Las flechas, la rueda sobre el vídeo, Inicio y Fin recorren el tipo de archivo abierto.

### Exportación

En **Editar → Convertir formato**, elige un formato disponible mediante los codecs integrados de KGen o los codificadores WIC instalados en Windows. La lista incluye formatos comunes como PNG, JPEG, BMP, TIFF, GIF, TGA, QOI, PNM, Radiance HDR, OpenEXR y Farbfeld, además de formatos que WIC aporte en ese equipo. La ventana permite guardar una copia sin cambiar el archivo abierto y muestra una animación mientras se convierte. En imágenes animadas o documentos multipágina se exporta el fotograma visible.

La opción **Guardar como** también ofrece los codificadores disponibles y conserva su comportamiento habitual: la imagen abierta pasa a usar el archivo guardado.

## Atajos de teclado

Activa **Opciones -> Activar captura rectangular** para habilitar la captura. Cada seleccion se copia como imagen al portapapeles y se guarda como PNG en la carpeta conocida **Imagenes\KGen** de Windows. Al terminar, KGen muestra una notificacion de Windows y reproduce el sonido de sistema. El marco cuadrado de 1 px usa dibujo GDI nativo; `Esc` cancela. En **Opciones -> Cambiar atajo de captura** puedes elegir otra combinacion, incluida la tecla Windows; **Iniciar capturas con Windows** es una opcion separada y esta desactivada inicialmente. El capturador residente no usa sondeo periodico. Si cierras KGen View mientras esta activo, el capturador permanece oculto; vuelve a abrir KGen View y desactivalo desde **Opciones** para detenerlo.

| Acción | Atajo |
|---|---|
| Abrir archivo | `Ctrl + O` |
| Guardar como | `Ctrl + Shift + S` |
| Archivo anterior / siguiente | `←` / `→` |
| Primer / último archivo de la carpeta | `Inicio` / `Fin` |
| Navegar con la rueda | `Rueda del ratón` |
| Acercar / alejar | `Ctrl + rueda`, `+` / `-` |
| Tamaño real | `1` |
| Encajar en la ventana | `F` |
| Recortar la última selección | `Ctrl + Shift + X` |
| Copiar imagen | `Ctrl + C` |
| Deshacer / rehacer | `Ctrl + Z` / `Ctrl + Y` |
| Fotograma o página anterior / siguiente | `RePág` / `AvPág` |
| Pausar o reanudar vídeo o animación | `Espacio` |
| Retroceder o avanzar 5 segundos en un vídeo | `RePág` / `AvPág` |
| Retroceder o avanzar 10 segundos en un vídeo | `J` / `L` |
| Avanzar un fotograma en vídeo | `E` |
| Saltar al inicio o al 10–90 % del vídeo | `0`–`9` |
| Subir o bajar el volumen del vídeo | `↑` / `↓` |
| Silenciar o recuperar el sonido del vídeo | `M` |
| Activar o desactivar el bucle del vídeo | `B` |
| Recorrer y activar los botones del reproductor | `Tab` / `Intro` |
| Presentación | `F5` |
| Recargar imagen | `F9` |
| Captura rectangular (si esta activada) | `Ctrl + Win + Z` (configurable) |
| Pantalla completa | `F11` |
| Salir de pantalla completa | `Esc` |
| Enviar a la Papelera | `Supr` |
| Información de la imagen | `I` |

## Instalación

Las instalaciones hechas con el instalador comprueban GitHub al abrirse. Cada subida a `main` publica una beta que se ofrece a todos; las etiquetas `vX.Y.Z` publican versiones estables. Si hay una versión nueva, pulsa el botón de descarga de la barra para descargarla y aplicarla. La app verifica el SHA-256 antes de abrir el instalador. Para actualizar la versión portátil, descarga y extrae el ZIP de la nueva publicación.

El instalador `KGenView-Setup-x64.exe` instala la aplicación en el perfil del usuario sin permisos de administrador y la ofrece en **Abrir con** para imágenes, ZIP, CBZ y formatos de vídeo como MP4, MKV, WebM, MOV y AVI.

El Explorador usa los iconos de KGen View cuando la aplicación está seleccionada como predeterminada para esa extensión. Al actualizar, Windows conserva las asociaciones anteriores. Si una extensión sigue mostrando el icono de su grupo, vuelve a seleccionar KGen View para esa extensión en **Configuración → Aplicaciones → Aplicaciones predeterminadas**.

Si estás usando una compilación local, el script de release genera estos archivos en `dist\`:

```text
dist\KGenView.exe
dist\KGenView-Portable-x64.zip
dist\KGenView-Setup-x64.exe
```

Para usar el vídeo sin instalar la aplicación, extrae `KGenView-Portable-x64.zip` y abre `KGenView.exe` con la carpeta `vlc` a su lado. El ejecutable suelto sigue disponible para abrir imágenes.

## Compilación desde el código fuente

### Requisitos

- Windows 10 u 11 de 64 bits.
- Rust estable con el target MSVC de 64 bits.
- Visual Studio Build Tools con las herramientas de C++ y el Windows SDK.
- Inno Setup 6 para generar el instalador.

La compilación de desarrollo solo necesita Rust y las herramientas de compilación de Windows. Inno Setup es necesario únicamente para crear el instalador.

### Ejecutar en desarrollo

```powershell
cargo run --release -- "C:\ruta\a\una\imagen.png"
```

Sin una ruta inicial, la aplicación se abre vacía y puedes cargar una imagen mediante el menú o arrastrándola a la ventana.

### Crear una release

GitHub Actions compila cada push a `main` y publica una beta. Para publicar una versión estable, actualiza `version` en `Cargo.toml`, crea la etiqueta correspondiente (`vX.Y.Z`) y súbela a GitHub. La etiqueta debe coincidir con la versión del proyecto; la publicación incluye el instalador, el ejecutable portable y los hashes SHA-256.

```powershell
.\scripts\build-release.ps1
```

El script ejecuta el formateador, Clippy y las pruebas antes de compilar la versión optimizada. Después copia el ejecutable a `dist\` y compila el instalador con Inno Setup.

Para firmar los artefactos de forma opcional, define estas variables de entorno antes de ejecutar el script:

```powershell
$env:KGEN_SIGN_CERT = "C:\ruta\certificado.pfx"
$env:KGEN_SIGN_PASSWORD = "tu-contraseña"
.\scripts\build-release.ps1
```

La firma requiere `signtool.exe`, incluido en el Windows SDK.

## Privacidad y red

KGen View no recopila telemetría. Al iniciar, las copias instaladas consultan las versiones publicadas en GitHub mediante HTTPS. Antes de instalar una actualización, el visor verifica el tamaño y el SHA-256 de la descarga y pide guardar los cambios pendientes. El instalador espera a que se cierre el proceso y, al terminar, reabre el archivo activo o el miembro del ZIP/CBZ. El Administrador de reinicio de Windows coordina las otras ventanas abiertas. Como todavía no hay un certificado Authenticode, el hash comprueba la integridad respecto al manifiesto publicado, pero no autentica por separado al editor. La app también usa la red cuando se selecciona **Copiar enlace directo (ImgBB)**.

Esta función necesita una clave API de ImgBB, que se configura desde **Información de imagen**. Al usarla, el archivo original se envía a `api.imgbb.com` mediante HTTPS y el enlace directo resultante se copia al portapapeles. ImgBB limita la subida a imágenes de hasta 32 MiB.

La configuración de la aplicación —incluida la clave de ImgBB— se guarda por usuario en el Registro de Windows, bajo:

```text
HKEY_CURRENT_USER\Software\KGenView
```

Si no necesitas compartir imágenes, puedes ignorar esta función y KGen View seguirá funcionando sin conexión.

## Control con IA mediante MCP

KGen View incluye un servidor [MCP](https://modelcontextprotocol.io/) local por **stdio**. Permite que un cliente MCP compatible controle una ventana dedicada de KGen View. No se inicia con el uso normal: el cliente debe lanzarlo expresamente con `--mcp`, por lo que no abre puertos ni añade tráfico de red.

Configura el cliente MCP para ejecutar el binario de KGen View con ese argumento. Por ejemplo, en clientes que usan `mcpServers`:

```json
{
  "mcpServers": {
    "kgen-view": {
      "command": "C:\\ruta\\a\\KGenView.exe",
      "args": ["--mcp"]
    }
  }
}
```

El cliente inicia una instancia de KGen View para esa sesión MCP. Las herramientas permiten consultar el estado, abrir una imagen por ruta absoluta, navegar por la carpeta o los fotogramas, ajustar la vista, alternar presentación o animación, aplicar rotaciones y volteos, y deshacer/rehacer. `convert_image` exporta una copia en un formato instalado en Windows; indica `output_path` y `format` (por ejemplo, `PNG`). La conversión se ejecuta en segundo plano, conserva abierta la imagen original y no sobrescribe un destino existente. No se exponen borrado, subida a ImgBB ni guardado automático de la imagen abierta. El cliente de IA puede recibir la ruta local y los metadatos de la imagen actual.

En el uso normal, sin `--mcp`, no se inicia el hilo MCP ni se enumeran formatos para esta función. La conversión solo se prepara tras una llamada MCP y usa el trabajador de operaciones de archivo ya existente; el hilo de interfaz sigue libre para dibujar y responder.

Como con cualquier servidor MCP, concede acceso solo a clientes de confianza: estos pueden solicitar la apertura de archivos locales y realizar las acciones enumeradas mientras la sesión esté activa.

## Arquitectura y rendimiento

KGen View mantiene una interfaz Win32 y renderizado Direct2D. La ventana coordina servicios de carga, catálogo, vigilancia de carpetas y operaciones de archivo; sus implementaciones son módulos Rust con visibilidad explícita.

- **Carga progresiva:** abre una vista ajustada y solicita detalle de la región visible al ampliar. JPEG aprovecha la reducción nativa de WIC cuando está disponible. Las dimensiones mostradas siguen siendo las originales.
- **Páginas bajo demanda:** TIFF carga una página cada vez. GIF conserva el estado de composición y una caché limitada de fotogramas, sin expandir toda la animación en RAM.
- **Edición geométrica:** rotación, volteo y recorte se almacenan como operaciones; deshacer/rehacer conserva instrucciones en lugar de copias completas. Guardar y convertir usan la resolución original.
- **Trabajo en segundo plano:** carga, precarga, catálogo, guardado, preparación del portapapeles, renombrado, Papelera y limpieza de ZIP. Los resultados tienen propiedad explícita en buzones acotados.
- **Renderizado:** texturas divididas en bloques y reutilizadas entre fotogramas compatibles; el repintado se produce por eventos. Minimizar pausa animación, presentación y precarga y libera cachés de presentación.
- **Carpetas:** listas compartidas con búsqueda indexada y notificaciones de cambios; la comprobación periódica queda como alternativa cuando Windows no permite vigilar la carpeta.
- **ZIP/CBZ:** navegación ligada al índice ordenado del archivo, lectores reutilizados por trabajador, extracción bajo demanda y temporales con vida útil ligada al visor o a una exportación.

El presupuesto por proceso permite reservar hasta **512 MiB de buffers CPU controlados** y **64 MiB de texturas de imagen**. La vista previa se limita a 32 MiB, la precarga a 16 MiB y la caché de fotogramas GIF a 32 MiB, subordinadas al presupuesto CPU común. Los códecs, Direct2D y sus controladores pueden realizar asignaciones internas adicionales: estos valores no son un límite absoluto de memoria del proceso.

Se conservan los límites de dimensiones de 100.000 × 100.000 píxeles, 512 páginas/fotogramas y 64 MiB de entrada SVG. La exportación devuelve un error cuando no puede conservar la resolución original dentro de los límites; no reduce silenciosamente el archivo guardado. JPEG XL usa hasta dos hilos de decodificación.

Consulta [docs/performance.md](docs/performance.md) para reproducir mediciones, revisar las decisiones de arquitectura y ejecutar las comprobaciones de calidad.

## Licencia

KGen View se distribuye bajo la licencia MIT. Consulta [LICENSE](LICENSE) para ver el texto completo.
