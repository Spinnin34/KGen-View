# Instrucciones del proyecto KGen View

## Publicación y actualizaciones

- Mantén operativo el publicador de `.github/workflows/windows.yml`: cada push a `main` pasa los controles de calidad, compila Windows y publica una beta en GitHub Releases. Las instalaciones siguen la beta más reciente y muestran el botón de actualización cuando hay una versión nueva.
- Las versiones estables se publican al subir una etiqueta `vX.Y.Z` que coincida exactamente con `version` en `Cargo.toml`. No cambies la versión para publicar betas.
- Cuando el usuario pida actualizar o corregir la app, conserva el flujo completo de actualización. Tras implementar y validar los cambios, publícalos en `main` y comprueba que Actions haya completado la compilación y la publicación, salvo que el usuario pida expresamente no publicar.
- Antes de publicar, conserva cambios locales previos y revisa el conjunto de archivos que se va a incluir. No añadas `.idea/`, secretos ni ejecutables locales de `dist/`; Actions genera los instaladores y manifiestos de release.
- Para validar localmente, ejecuta `cargo fmt -- --check` y `cargo check --locked`. La validación de GitHub Actions también ejecuta Clippy, tests y una compilación optimizada; no omitas ese workflow al publicar.

## Integridad del actualizador

- Conserva `schema_version: 1` del manifiesto, el repositorio `Spinnin34/KGen-View`, las URLs HTTPS de GitHub permitidas y la verificación SHA-256 del instalador antes de ejecutarlo.
- Mantén el actualizador fuera del hilo de interfaz, limita tamaños y tiempos de descarga y no ejecutes una descarga cuya huella o tamaño no coincidan.
- Conserva las imágenes abiertas al actualizar: cada proceso/ventana registra su archivo actual con `RegisterApplicationRestart`, y las selecciones de ZIP deben restaurar archivo y miembro. Al cambiar la carga, navegación o ruta guardada, actualiza también los argumentos de reinicio.
- La publicación actual no tiene firma Authenticode: HTTPS y SHA-256 detectan corrupción, pero no autentican al publicador de forma independiente. No afirmes lo contrario.
- Las copias portables no reciben instalación automática; documenta cualquier cambio de este comportamiento.
