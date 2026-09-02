# KGen View

<p align="center">
  <img src="resources/kgen-view-logo.png" alt="KGen View" width="96">
</p>

<p align="center">
  Visor de imágenes nativo para Windows, rápido, compacto y sin distracciones.
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
- Navegación circular por las imágenes de la carpeta, con orden natural para nombres como `imagen2.png` e `imagen10.png`.
- Redimensionado automático de la ventana para aprovechar el espacio disponible.
- Zoom con rueda, zoom libre, tamaño real, ajuste a ventana y ajuste al ancho.
- Paneo y pantalla completa.
- GIF animado y navegación por fotogramas o páginas en documentos multipágina compatibles.
- Rotación de 90°, volteo horizontal y vertical, recorte de selección y deshacer/rehacer.
- Guardar como PNG, JPEG, BMP o TIFF.
- Copiar la imagen o su ruta al portapapeles.
- Envío a la Papelera de reciclaje con confirmación configurable.
- Presentación con intervalo configurable.
- Instancia única: al abrir otra imagen, la ventana existente recibe el archivo.
- Salto automático de archivos dañados o incompatibles durante la navegación.
- Sin telemetría, WebView ni procesos auxiliares.

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
| HEIF / HEIC | `.heif`, `.heic` |
| SVG | `.svg` |
| Otros codecs WIC | `.wdp`, `.jxr`, `.hdp`, `.dds`, si el codec está disponible en Windows |

WebP y HEIF/HEIC dependen de los codecs instalados en Windows. La aplicación muestra un aviso cuando el sistema no puede decodificar uno de estos formatos.

### Exportación

La opción **Guardar como** permite exportar el fotograma visible a PNG, JPEG, BMP o TIFF. Al guardar una animación o un documento multipágina se exporta el fotograma que está actualmente en pantalla.

## Atajos de teclado

| Acción | Atajo |
|---|---|
| Abrir imagen | `Ctrl + O` |
| Guardar como | `Ctrl + Shift + S` |
| Imagen anterior / siguiente | `←` / `→` |
| Primera / última imagen de la carpeta | `Inicio` / `Fin` |
| Navegar con la rueda | `Rueda del ratón` |
| Acercar / alejar | `Ctrl + rueda`, `+` / `-` |
| Tamaño real | `1` |
| Encajar en la ventana | `F` |
| Recortar la última selección | `Ctrl + Shift + X` |
| Copiar imagen | `Ctrl + C` |
| Deshacer / rehacer | `Ctrl + Z` / `Ctrl + Y` |
| Fotograma o página anterior / siguiente | `RePág` / `AvPág` |
| Pausar o reanudar animación | `Espacio` |
| Presentación | `F5` |
| Recargar imagen | `F9` |
| Pantalla completa | `F11` |
| Salir de pantalla completa | `Esc` |
| Enviar a la Papelera | `Supr` |
| Información de la imagen | `I` |

## Instalación

La forma recomendada para distribuir KGen View es el instalador `KGenView-Setup-x64.exe`. El instalador no necesita permisos de administrador: se instala en el perfil del usuario y puede registrar las asociaciones de archivos para BMP, JPEG, PNG, GIF, TIFF, ICO, WebP, HEIF/HEIC y SVG.

Si estás usando una compilación local, el script de release genera estos archivos en `dist\`:

```text
dist\KGenView.exe
dist\KGenView-Setup-x64.exe
```

También puedes ejecutar `KGenView.exe` como versión portable sin instalarlo.

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

KGen View no recopila telemetría ni realiza conexiones en segundo plano. La red solo se utiliza cuando se selecciona **Copiar enlace directo (ImgBB)**.

Esta función necesita una clave API de ImgBB, que se configura desde **Información de imagen**. Al usarla, el archivo original se envía a `api.imgbb.com` mediante HTTPS y el enlace directo resultante se copia al portapapeles. ImgBB limita la subida a imágenes de hasta 32 MiB.

La configuración de la aplicación —incluida la clave de ImgBB— se guarda por usuario en el Registro de Windows, bajo:

```text
HKEY_CURRENT_USER\Software\KGenView
```

Si no necesitas compartir imágenes, puedes ignorar esta función y KGen View seguirá funcionando sin conexión.

## Arquitectura

El código está organizado por responsabilidades para mantener la aplicación pequeña y fácil de mantener:

```text
src/
├── app.rs                         Fachada de la aplicación
├── app/controller.rs              Estado de la ventana y flujo principal
├── app/controller/loader.rs       Carga y precarga en segundo plano
├── catalog.rs                     Catálogo y navegación de carpetas
├── commands.rs                    Comandos compartidos por menús y atajos
├── imaging.rs                     WIC, GIF, SVG, metadatos, edición e historial
├── settings.rs                    Preferencias por usuario
├── ui/                            Layout, tema, barra de herramientas e iconos
└── platform/                      Adaptadores específicos de Win32
```

El cargador usa una cola coalescente y control por generaciones: si el usuario recorre una carpeta rápidamente, se conserva la petición más reciente y se descartan resultados obsoletos. La precarga mantiene únicamente los recursos necesarios para que la navegación sea fluida sin dejar crecer la memoria indefinidamente.

Para evitar cargas accidentales desproporcionadas, el decodificador limita las dimensiones de una imagen a `100.000 × 100.000` píxeles, el presupuesto de memoria decodificada a 512 MiB y los documentos multipágina a 512 fotogramas.

## Licencia

KGen View se distribuye bajo la licencia MIT. Consulta [LICENSE](LICENSE) para ver el texto completo.

