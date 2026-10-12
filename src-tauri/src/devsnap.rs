//! Debug-only webview snapshots (feature `dev-snapshot`, macOS).

#[tauri::command]
pub async fn dev_snapshot(window: tauri::WebviewWindow, path: String) -> Result<(), String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<Vec<u8>, String>>();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
    window
        .with_webview(move |wv| unsafe {
            use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
            use objc2_foundation::{NSDictionary, NSError};
            let wk: &objc2_web_kit::WKWebView = &*(wv.inner() as *const objc2_web_kit::WKWebView);
            let tx2 = tx.clone();
            let block = block2::RcBlock::new(move |img: *mut NSImage, _err: *mut NSError| {
                let res = (|| {
                    let img = img.as_ref().ok_or("no image")?;
                    let tiff = img.TIFFRepresentation().ok_or("no tiff")?;
                    let rep = NSBitmapImageRep::imageRepWithData(&tiff).ok_or("no rep")?;
                    let png = rep
                        .representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
                        .ok_or("no png")?;
                    Ok::<Vec<u8>, &str>(png.to_vec())
                })();
                if let Some(t) = tx2.lock().unwrap().take() {
                    let _ = t.send(res.map_err(|e| e.to_string()));
                }
            });
            wk.takeSnapshotWithConfiguration_completionHandler(None, &block);
        })
        .map_err(|e| e.to_string())?;
    let png = rx.await.map_err(|e| e.to_string())??;
    std::fs::write(&path, png).map_err(|e| e.to_string())
}
