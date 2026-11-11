use super::*;
use serde_json::{Value, json};

impl AppState {
    pub(super) fn handle_mcp_request(
        &mut self,
        message: &Value,
        reply: std::sync::mpsc::Sender<Value>,
    ) -> Option<Value> {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        match method {
            "initialize" => {
                let requested = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or("2025-03-26");
                if !["2025-06-18", "2025-03-26", "2024-11-05"].contains(&requested) {
                    return Some(crate::mcp::error(
                        id,
                        -32602,
                        format!("Unsupported MCP protocol version: {requested}"),
                    ));
                }
                Some(crate::mcp::response(
                    id,
                    json!({
                        "protocolVersion": requested,
                        "capabilities": {"tools": {"listChanged": false}},
                        "serverInfo": {"name":"kgen-view","version":env!("CARGO_PKG_VERSION")},
                        "instructions":"Control of the local KGen View window. File paths and current image metadata are shared with the connected AI client. Mutating tools are limited to reversible viewing and editing actions."
                    }),
                ))
            }
            "ping" => Some(crate::mcp::response(id, json!({}))),
            "tools/list" => Some(crate::mcp::response(id, json!({"tools": tools()}))),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                if name == "convert_image" {
                    return match self.queue_mcp_export(&args) {
                        Ok(()) => {
                            self.pending_mcp_reply = Some((id, reply));
                            None
                        }
                        Err(message) => Some(crate::mcp::response(
                            id,
                            json!({"content":[{"type":"text","text":message}],"isError":true}),
                        )),
                    };
                }
                Some(match self.call_tool(name, &args) {
                    Ok(value) => crate::mcp::response(id, crate::mcp::text_result(value)),
                    Err(message) => crate::mcp::response(
                        id,
                        json!({"content":[{"type":"text","text":message}],"isError":true}),
                    ),
                })
            }
            _ => Some(crate::mcp::error(
                id,
                -32601,
                format!("Method not found: {method}"),
            )),
        }
    }

    fn queue_mcp_export(&mut self, args: &Value) -> Result<(), String> {
        if self.document.is_none() {
            return Err("Open an image before converting it.".into());
        }
        if self.file_busy {
            return Err("Another file operation is already in progress.".into());
        }
        let raw_path = args
            .get("output_path")
            .and_then(Value::as_str)
            .ok_or("Required argument: output_path")?;
        let raw_format = args
            .get("format")
            .and_then(Value::as_str)
            .ok_or("Required argument: format")?;
        let mut path = PathBuf::from(raw_path);
        if !path.is_absolute() {
            return Err("Output path must be absolute.".into());
        }
        if !self.ensure_export_formats() {
            return Err("No compatible image encoders are available on this computer.".into());
        }
        let formats = self.export_formats.as_ref().unwrap();
        let format = formats
            .iter()
            .find(|candidate| {
                candidate.name.eq_ignore_ascii_case(raw_format)
                    || candidate.extensions.iter().any(|extension| {
                        extension.eq_ignore_ascii_case(raw_format.trim_start_matches('.'))
                    })
            })
            .cloned()
            .ok_or_else(|| format!("Unsupported output format: {raw_format}"))?;
        match path.extension().and_then(|extension| extension.to_str()) {
            None => {
                path.set_extension(format.extension());
            }
            Some(extension) if !format.accepts_extension(extension) => {
                return Err(format!(
                    "Output path extension .{extension} does not match format {}.",
                    format.name
                ));
            }
            _ => {}
        }
        if path.exists() {
            return Err(
                "Output file already exists; choose a new path to avoid overwriting it.".into(),
            );
        }
        if let Some(parent) = path.parent()
            && !parent.is_dir()
        {
            return Err("Output directory does not exist.".into());
        }
        let task = FileTask::ExportCopy {
            document: self.document.as_ref().unwrap().export_snapshot(),
            path,
            format,
        };
        if !self.file_service.submit(task) {
            return Err("Another file operation is already in progress.".into());
        }
        self.file_busy = true;
        self.update_status(Some("Convirtiendo una copia en segundo plano…"));
        Ok(())
    }

    pub(super) fn complete_mcp_export(&mut self, result: Result<PathBuf, String>) {
        let Some((id, reply)) = self.pending_mcp_reply.take() else {
            return;
        };
        let response = match result {
            Ok(path) => crate::mcp::response(
                id,
                crate::mcp::text_result(json!({
                    "created": true,
                    "output_path": path.to_string_lossy(),
                    "format": path.extension().and_then(|value| value.to_str()).unwrap_or("")
                })),
            ),
            Err(message) => crate::mcp::response(
                id,
                json!({"content":[{"type":"text","text":message}],"isError":true}),
            ),
        };
        let _ = reply.send(response);
    }

    fn call_tool(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let command = match name {
            "get_status" => return Ok(self.status_json()),
            "open_image" => {
                let raw = args
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or("Required argument: path")?;
                let path = PathBuf::from(raw);
                if !path.is_absolute() {
                    return Err("Image path must be absolute.".into());
                }
                if !path.is_file() {
                    return Err("Image file does not exist or is not a regular file.".into());
                }
                self.start_load(path);
                return Ok(json!({"accepted":true,"message":"Image load started."}));
            }
            "next_image" => cmd::NEXT,
            "previous_image" => cmd::PREVIOUS,
            "rotate_left" => cmd::ROTATE_LEFT,
            "rotate_right" => cmd::ROTATE_RIGHT,
            "flip_horizontal" => cmd::FLIP_H,
            "flip_vertical" => cmd::FLIP_V,
            "undo" => cmd::UNDO,
            "redo" => cmd::REDO,
            "fit_to_window" => cmd::FIT,
            "fit_to_width" => cmd::FIT_WIDTH,
            "actual_size" => cmd::ACTUAL,
            "toggle_fullscreen" => cmd::FULLSCREEN,
            "toggle_slideshow" => cmd::SLIDESHOW,
            "toggle_animation" => cmd::PAUSE_ANIMATION,
            "next_frame" => cmd::NEXT_FRAME,
            "previous_frame" => cmd::PREVIOUS_FRAME,
            _ => return Err(format!("Unknown tool: {name}")),
        };
        self.handle_command(command);
        Ok(json!({"accepted":true,"action":name}))
    }

    fn status_json(&self) -> Value {
        let document = self.document.as_ref();
        let dimensions = document.map(|doc| doc.view_dimensions());
        json!({
            "loaded": document.is_some(),
            "path": document.map(|doc| doc.path.to_string_lossy().to_string()),
            "format": document.map(|doc| doc.format_name.as_str()),
            "width": dimensions.map(|size| size.0),
            "height": dimensions.map(|size| size.1),
            "frame": document.map(|_| self.requested_page + 1),
            "frame_count": document.map(|doc| doc.frame_count()),
            "zoom": self.zoom,
            "dirty": document.is_some_and(|doc| doc.dirty),
            "loading": self.load_started.is_some(),
            "fullscreen": self.fullscreen,
            "slideshow": self.slideshow,
            "animation_playing": self.animation_playing,
        })
    }
}

fn tools() -> Vec<Value> {
    let mut list = vec![
        tool(
            "get_status",
            "Get the current image, dimensions, frame, zoom, and viewer state.",
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ),
        tool(
            "open_image",
            "Open a local image file by its absolute path.",
            json!({"type":"object","properties":{"path":{"type":"string","description":"Absolute local image path"}},"required":["path"],"additionalProperties":false}),
        ),
        tool(
            "convert_image",
            "Export the current image to a new file in a Windows-supported format. The output path must not already exist; the original image remains open and unchanged.",
            json!({"type":"object","properties":{"output_path":{"type":"string","description":"Absolute new file path. Its extension must match format, or may be omitted."},"format":{"type":"string","description":"Available format name or extension, such as PNG, JPEG, or TIFF."}},"required":["output_path","format"],"additionalProperties":false}),
        ),
    ];
    for (name, description) in [
        ("next_image", "Show the next image in the current folder."),
        (
            "previous_image",
            "Show the previous image in the current folder.",
        ),
        (
            "rotate_left",
            "Rotate the current image 90 degrees counterclockwise (undoable).",
        ),
        (
            "rotate_right",
            "Rotate the current image 90 degrees clockwise (undoable).",
        ),
        (
            "flip_horizontal",
            "Flip the current image horizontally (undoable).",
        ),
        (
            "flip_vertical",
            "Flip the current image vertically (undoable).",
        ),
        ("undo", "Undo the most recent image edit."),
        ("redo", "Redo the most recently undone image edit."),
        ("fit_to_window", "Fit the image inside the viewer window."),
        ("fit_to_width", "Fit the image to the viewer width."),
        ("actual_size", "Show the image at actual pixel size."),
        ("toggle_fullscreen", "Toggle fullscreen mode."),
        ("toggle_slideshow", "Start or stop the slideshow."),
        ("toggle_animation", "Pause or resume the current animation."),
        (
            "next_frame",
            "Show the next frame or page of the current image.",
        ),
        (
            "previous_frame",
            "Show the previous frame or page of the current image.",
        ),
    ] {
        list.push(tool(
            name,
            description,
            json!({"type":"object","properties":{},"additionalProperties":false}),
        ));
    }
    list
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    let read_only = name == "get_status";
    json!({
        "name":name,
        "description":description,
        "inputSchema":input_schema,
        "annotations": {
            "readOnlyHint":read_only,
            "destructiveHint":name == "convert_image",
            "idempotentHint":read_only || matches!(name, "fit_to_window" | "fit_to_width" | "actual_size"),
            "openWorldHint":name == "open_image" || name == "convert_image"
        }
    })
}
