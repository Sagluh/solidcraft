package dev.solidcraft.matepad;

import android.app.NativeActivity;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.provider.OpenableColumns;
import android.util.Log;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;

/**
 * Android Storage Access Framework bridge for the native Rust CAD editor.
 *
 * All engine I/O stays in the app's private files. Android owns document Uri
 * permissions and copies selected imports in. Exports are copied back to the
 * destination Uri once the Rust save operation has completed.
 */
public final class MatePadActivity extends NativeActivity {
    private static final int PICK_OPEN = 8101;
    private static final int PICK_SAVE = 8102;
    private static final String CANCELLED = "__SOLIDCRAFT_CANCELLED__";
    private volatile String pickerResult = "";
    private volatile boolean pickerActive = false;
    private volatile Uri saveUri = null;
    private volatile String saveLocalPath = null;

    /** Called from Rust. Schedules SAF on Android's UI thread. */
    public void launchFilePicker(final int mode, final String suggestedName) {
        runOnUiThread(() -> {
            try {
                if (pickerActive) {
                    pickerResult = CANCELLED;
                    return;
                }
                pickerActive = true;
                pickerResult = "";
                Intent intent = new Intent(
                    mode == 1 ? Intent.ACTION_OPEN_DOCUMENT : Intent.ACTION_CREATE_DOCUMENT);
                intent.addCategory(Intent.CATEGORY_OPENABLE);
                intent.setType("*/*");
                intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION |
                                Intent.FLAG_GRANT_WRITE_URI_PERMISSION |
                                Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
                if (mode == 2) {
                    String title = sanitize(suggestedName);
                    if (title.isEmpty()) title = "Design.solidcraft";
                    intent.putExtra(Intent.EXTRA_TITLE, title);
                    intent.setType(mimeType(title));
                }
                startActivityForResult(intent, mode == 1 ? PICK_OPEN : PICK_SAVE);
            } catch (Exception error) {
                Log.e("SolidCraft", "Cannot open system document picker", error);
                pickerActive = false;
                pickerResult = CANCELLED;
            }
        });
    }

    /** Called periodically by Rust's async FilePickerFuture. Empty means pending. */
    public synchronized String takeFilePickerResult() {
        String result = pickerResult;
        pickerResult = "";
        return result;
    }

    /** Path whose latest completed bytes should be written to the chosen SAF Uri. */
    public String getPendingSavePath() {
        return saveLocalPath == null ? "" : saveLocalPath;
    }

    /**
     * Copies a completed native save or export into the user-selected file.
     * This method runs on the native worker thread, not the Android UI thread.
     */
    public synchronized boolean commitSavedFile(String path) {
        if (path == null || saveUri == null || !path.equals(saveLocalPath)) return false;
        File from = new File(path);
        if (!from.isFile() || !from.canRead()) return false;
        try (InputStream input = new FileInputStream(from);
             OutputStream output = getContentResolver().openOutputStream(saveUri, "wt")) {
            if (output == null) return false;
            byte[] chunk = new byte[65536];
            int count;
            while ((count = input.read(chunk)) != -1) output.write(chunk, 0, count);
            output.flush();
            return true;
        } catch (Exception error) {
            Log.e("SolidCraft", "Failed to write exported file through SAF", error);
            return false;
        }
    }

    @Override
    protected void onActivityResult(int code, int resultCode, Intent data) {
        super.onActivityResult(code, resultCode, data);
        if (code != PICK_OPEN && code != PICK_SAVE) return;
        String result = CANCELLED;
        try {
            Uri uri = resultCode == RESULT_OK && data != null ? data.getData() : null;
            if (uri != null) {
                if (code == PICK_OPEN) {
                    String name = getDocumentName(uri);
                    File folder = new File(getFilesDir(), "imports");
                    if (!folder.exists() && !folder.mkdirs()) throw new Exception("Cannot create imports");
                    File local = new File(folder, System.nanoTime() + "-" + sanitize(name));
                    try (InputStream input = getContentResolver().openInputStream(uri);
                         OutputStream output = new FileOutputStream(local)) {
                        if (input == null) throw new Exception("Cannot read selected file");
                        byte[] chunk = new byte[65536];
                        int n;
                        while ((n = input.read(chunk)) != -1) output.write(chunk, 0, n);
                    }
                    result = local.getAbsolutePath();
                } else {
                    String name = getDocumentName(uri);
                    File folder = new File(getFilesDir(), "exports");
                    if (!folder.exists() && !folder.mkdirs()) throw new Exception("Cannot create exports");
                    File local = new File(folder, sanitize(name));
                    // Delete a previous local copy to avoid exporting stale data.
                    if (local.exists() && !local.delete()) throw new Exception("Cannot reset export");
                    saveUri = uri;
                    saveLocalPath = local.getAbsolutePath();
                    result = saveLocalPath;
                }
            }
        } catch (Exception error) {
            Log.e("SolidCraft", "System file picker operation failed", error);
        }
        synchronized (this) {
            pickerResult = result;
            pickerActive = false;
        }
    }

    private String getDocumentName(Uri uri) {
        String name = null;
        try (Cursor cursor = getContentResolver().query(
                uri, new String[]{OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                name = cursor.getString(0);
            }
        } catch (Exception ignored) {
            // Some document providers do not expose display names.
        }
        if (name == null || name.isEmpty()) name = "document.solidcraft";
        return sanitize(name);
    }

    private static String sanitize(String name) {
        if (name == null) return "";
        String clean = name.replace("/", "_").replace("\\", "_").replace("\0", "_").trim();
        if (clean.equals(".") || clean.equals("..")) return "document.solidcraft";
        return clean;
    }

    private static String mimeType(String name) {
        String lower = name.toLowerCase(java.util.Locale.ROOT);
        if (lower.endsWith(".json")) return "application/json";
        if (lower.endsWith(".png")) return "image/png";
        if (lower.endsWith(".svg")) return "image/svg+xml";
        if (lower.endsWith(".dxf")) return "application/dxf";
        if (lower.endsWith(".csv")) return "text/csv";
        return "application/octet-stream";
    }
}