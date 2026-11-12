#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    Spanish,
    English,
}

impl Language {
    pub fn detect() -> Self {
        let language = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
        if language & 0x03ff == 0x000a {
            Self::Spanish
        } else {
            Self::English
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Spanish => Self::English,
            Self::English => Self::Spanish,
        }
    }
}

#[derive(Clone, Copy)]
pub enum Text {
    AppTitle,
    File,
    Edit,
    Image,
    View,
    Options,
    Help,
    Open,
    SaveAs,
    Exit,
    Undo,
    Redo,
    Copy,
    Crop,
    Delete,
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
    Previous,
    Next,
    ActualSize,
    Fit,
    FitWidth,
    Fullscreen,
    Slideshow,
    PauseAnimation,
    PreviousPage,
    NextPage,
    Reload,
    Information,
    OpenFolder,
    CopyPath,
    LanguageMenu,
    Theme,
    About,
    Loading,
    NoImage,
    Unsupported,
    CodecMissing,
    DownloadUpdate,
    DownloadingUpdate,
    InstallingUpdate,
    SaveChanges,
    ConfirmDelete,
    Error,
}

pub fn tr(lang: Language, key: Text) -> &'static str {
    use Text::*;
    match (lang, key) {
        (Language::Spanish, AppTitle) => "KGen View",
        (Language::Spanish, File) => "Archivo",
        (Language::Spanish, Edit) => "Editar",
        (Language::Spanish, Image) => "Imagen",
        (Language::Spanish, View) => "Ver",
        (Language::Spanish, Options) => "Opciones",
        (Language::Spanish, Help) => "Ayuda",
        (Language::Spanish, Open) => "Abrir...\tCtrl+O",
        (Language::Spanish, SaveAs) => "Guardar como...\tCtrl+Mayús+S",
        (Language::Spanish, Exit) => "Salir",
        (Language::Spanish, Undo) => "Deshacer\tCtrl+Z",
        (Language::Spanish, Redo) => "Rehacer\tCtrl+Y",
        (Language::Spanish, Copy) => "Copiar imagen\tCtrl+C",
        (Language::Spanish, Crop) => "Recortar selección\tCtrl+Mayús+X",
        (Language::Spanish, Delete) => "Enviar a la Papelera\tSupr",
        (Language::Spanish, RotateLeft) => "Girar 90° a la izquierda",
        (Language::Spanish, RotateRight) => "Girar 90° a la derecha",
        (Language::Spanish, FlipHorizontal) => "Voltear horizontalmente",
        (Language::Spanish, FlipVertical) => "Voltear verticalmente",
        (Language::Spanish, Previous) => "Anterior",
        (Language::Spanish, Next) => "Siguiente",
        (Language::Spanish, ActualSize) => "Tamaño real\t1",
        (Language::Spanish, Fit) => "Encajar\tF",
        (Language::Spanish, FitWidth) => "Ajustar al ancho",
        (Language::Spanish, Fullscreen) => "Pantalla completa\tF11",
        (Language::Spanish, Slideshow) => "Presentación\tF5",
        (Language::Spanish, PauseAnimation) => "Pausar/reanudar animación\tEspacio",
        (Language::Spanish, PreviousPage) => "Página/fotograma anterior\tRePág",
        (Language::Spanish, NextPage) => "Página/fotograma siguiente\tAvPág",
        (Language::Spanish, Reload) => "Recargar\tF9",
        (Language::Spanish, Information) => "Información de imagen\tI",
        (Language::Spanish, OpenFolder) => "Abrir carpeta contenedora",
        (Language::Spanish, CopyPath) => "Copiar ruta",
        (Language::Spanish, LanguageMenu) => "Idioma: Español / English",
        (Language::Spanish, Theme) => "Alternar tema claro/oscuro",
        (Language::Spanish, About) => "Acerca de KGen View",
        (Language::Spanish, Loading) => "Cargando…",
        (Language::Spanish, NoImage) => "Abre o arrastra una imagen",
        (Language::Spanish, Unsupported) => "No se pudo abrir la imagen.",
        (Language::Spanish, CodecMissing) => "Puede faltar el codec de Windows para este formato.",
        (Language::Spanish, DownloadUpdate) => "Descargar actualización",
        (Language::Spanish, DownloadingUpdate) => "Descargando actualización…",
        (Language::Spanish, InstallingUpdate) => "Instalando actualización…",
        (Language::Spanish, SaveChanges) => {
            "La imagen tiene cambios sin guardar. ¿Quieres guardarlos?"
        }
        (Language::Spanish, ConfirmDelete) => "¿Enviar esta imagen a la Papelera?",
        (Language::Spanish, Error) => "Error",

        (Language::English, AppTitle) => "KGen View",
        (Language::English, File) => "File",
        (Language::English, Edit) => "Edit",
        (Language::English, Image) => "Image",
        (Language::English, View) => "View",
        (Language::English, Options) => "Options",
        (Language::English, Help) => "Help",
        (Language::English, Open) => "Open...\tCtrl+O",
        (Language::English, SaveAs) => "Save as...\tCtrl+Shift+S",
        (Language::English, Exit) => "Exit",
        (Language::English, Undo) => "Undo\tCtrl+Z",
        (Language::English, Redo) => "Redo\tCtrl+Y",
        (Language::English, Copy) => "Copy image\tCtrl+C",
        (Language::English, Crop) => "Crop selection\tCtrl+Shift+X",
        (Language::English, Delete) => "Move to Recycle Bin\tDel",
        (Language::English, RotateLeft) => "Rotate 90° left",
        (Language::English, RotateRight) => "Rotate 90° right",
        (Language::English, FlipHorizontal) => "Flip horizontally",
        (Language::English, FlipVertical) => "Flip vertically",
        (Language::English, Previous) => "Previous",
        (Language::English, Next) => "Next",
        (Language::English, ActualSize) => "Actual size\t1",
        (Language::English, Fit) => "Fit\tF",
        (Language::English, FitWidth) => "Fit width",
        (Language::English, Fullscreen) => "Fullscreen\tF11",
        (Language::English, Slideshow) => "Slideshow\tF5",
        (Language::English, PauseAnimation) => "Pause/resume animation\tSpace",
        (Language::English, PreviousPage) => "Previous page/frame\tPage Up",
        (Language::English, NextPage) => "Next page/frame\tPage Down",
        (Language::English, Reload) => "Reload\tF9",
        (Language::English, Information) => "Image information\tI",
        (Language::English, OpenFolder) => "Open containing folder",
        (Language::English, CopyPath) => "Copy path",
        (Language::English, LanguageMenu) => "Language: Español / English",
        (Language::English, Theme) => "Toggle light/dark theme",
        (Language::English, About) => "About KGen View",
        (Language::English, Loading) => "Loading…",
        (Language::English, NoImage) => "Open or drop an image",
        (Language::English, Unsupported) => "The image could not be opened.",
        (Language::English, CodecMissing) => "The Windows codec for this format may be missing.",
        (Language::English, DownloadUpdate) => "Download update",
        (Language::English, DownloadingUpdate) => "Downloading update…",
        (Language::English, InstallingUpdate) => "Installing update…",
        (Language::English, SaveChanges) => {
            "This image has unsaved changes. Do you want to save them?"
        }
        (Language::English, ConfirmDelete) => "Move this image to the Recycle Bin?",
        (Language::English, Error) => "Error",
    }
}
