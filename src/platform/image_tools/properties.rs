use super::*;

pub(super) const CLIENT: (i32, i32) = (560, 552);
const FILE_NAME: i32 = 400;
const AUTHOR: i32 = 401;
const DESCRIPTION: i32 = 402;
const API_KEY: i32 = 403;

pub struct PropertiesData {
    pub file_name: String,
    pub folder: String,
    pub full_path: String,
    pub format: String,
    pub resolution: String,
    pub original_size: String,
    pub current_size: String,
    pub print_size: String,
    pub current_colors: String,
    pub unique_colors: String,
    pub disk_size: String,
    pub memory_size: String,
    pub folder_index: String,
    pub file_time: String,
    pub load_time: String,
    pub author: String,
    pub description: String,
    pub api_key: String,
}

pub struct PropertiesEdit {
    pub file_name: String,
    pub author: String,
    pub description: String,
    pub api_key: String,
}

pub(super) struct Panel {
    data: PropertiesData,
    pub result: Option<PropertiesEdit>,
}

impl Panel {
    pub fn new(data: PropertiesData) -> Self {
        Self { data, result: None }
    }

    pub fn create(&self, hwnd: HWND, d: &Dialog) -> HWND {
        let data = &self.data;
        d.group(hwnd, d.text("Archivo", "File"), [8, 5, 544, 112]);
        d.group(
            hwnd,
            d.text("Datos de la imagen", "Image details"),
            [8, 121, 544, 270],
        );
        d.group(
            hwnd,
            d.text("Campos editables", "Editable fields"),
            [8, 395, 544, 113],
        );

        let filename = field(
            hwnd,
            d,
            FILE_NAME,
            d.text("Nombre de archivo", "File name"),
            &data.file_name,
            23,
            20,
            false,
            true,
        );
        field(
            hwnd,
            d,
            0,
            d.text("Carpeta", "Folder"),
            &data.folder,
            47,
            20,
            true,
            true,
        );
        field(
            hwnd,
            d,
            0,
            d.text("Ruta completa", "Full path"),
            &data.full_path,
            71,
            38,
            true,
            true,
        );
        for (index, (label, value)) in [
            (d.text("Formato", "Format"), &data.format),
            (d.text("Resolución", "Resolution"), &data.resolution),
            (
                d.text("Tamaño original", "Original size"),
                &data.original_size,
            ),
            (d.text("Tamaño actual", "Current size"), &data.current_size),
            (
                d.text("Tamaño de impresión", "Print size"),
                &data.print_size,
            ),
            (
                d.text("Formato de píxel", "Pixel format"),
                &data.current_colors,
            ),
            (
                d.text("Colores únicos", "Unique colors"),
                &data.unique_colors,
            ),
            (d.text("Tamaño en disco", "Disk size"), &data.disk_size),
            (
                d.text("Memoria decodificada", "Decoded memory"),
                &data.memory_size,
            ),
            (
                d.text("Posición en carpeta", "Folder position"),
                &data.folder_index,
            ),
            (
                d.text("Fecha del archivo", "File date and time"),
                &data.file_time,
            ),
            (d.text("Tiempo de carga", "Load time"), &data.load_time),
        ]
        .into_iter()
        .enumerate()
        {
            field(
                hwnd,
                d,
                0,
                label,
                value,
                141 + index as i32 * 20,
                19,
                true,
                false,
            );
        }
        field(
            hwnd,
            d,
            AUTHOR,
            d.text("Autor", "Author"),
            &data.author,
            413,
            20,
            false,
            true,
        );
        field(
            hwnd,
            d,
            DESCRIPTION,
            d.text("Descripción", "Description"),
            &data.description,
            437,
            38,
            false,
            true,
        );
        field(
            hwnd,
            d,
            API_KEY,
            d.text("Clave API ImgBB", "ImgBB API key"),
            &data.api_key,
            479,
            20,
            false,
            true,
        );
        d.button(
            hwnd,
            ACCEPT,
            d.text("Guardar", "Save"),
            BS_DEFPUSHBUTTON,
            [360, 519, 90, 25],
        );
        d.button(
            hwnd,
            CANCEL,
            d.text("Cancelar", "Cancel"),
            BS_PUSHBUTTON,
            [462, 519, 90, 25],
        );
        filename
    }

    pub fn command(&mut self, hwnd: HWND, id: i32) -> bool {
        if id != ACCEPT {
            return false;
        }
        self.result = Some(PropertiesEdit {
            file_name: read(hwnd, FILE_NAME),
            author: read(hwnd, AUTHOR),
            description: read(hwnd, DESCRIPTION),
            api_key: read(hwnd, API_KEY),
        });
        true
    }
}

fn field(
    hwnd: HWND,
    d: &Dialog,
    id: i32,
    label: &str,
    value: &str,
    y: i32,
    height: i32,
    read_only: bool,
    border: bool,
) -> HWND {
    d.label(hwnd, 0, label, [18, y + 2, 141, 18]);
    let mut style = WS_TABSTOP;
    if border {
        style |= WS_BORDER;
    }
    if read_only {
        style |= WINDOW_STYLE(ES_READONLY as u32);
    }
    if height > 20 {
        style |= WS_VSCROLL | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32);
    } else {
        style |= WINDOW_STYLE(ES_AUTOHSCROLL as u32);
    }
    d.control(hwnd, id, w!("EDIT"), value, style, [165, y, 376, height])
}

fn read(hwnd: HWND, id: i32) -> String {
    let edit = control(hwnd, id);
    let length = unsafe { GetWindowTextLengthW(edit) }.max(0) as usize;
    let mut buffer = vec![0; length + 1];
    let read = unsafe { GetWindowTextW(edit, &mut buffer) }.max(0) as usize;
    String::from_utf16_lossy(&buffer[..read])
}
