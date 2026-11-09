//! Form view of the config editor: Tauri commands over `pinscape_config::form`
//! (the form model) and `pinscape_config::edit` (minimal text edits). The
//! text stays the source of truth: the UI applies the returned edits to the
//! editor, so comments and formatting survive and undo works.

use pinscape_config::edit::{self, EditOp, EditResult};
use pinscape_config::form::{self, FormView};

/// The form for one top-level section (`None`: the first common one in the file).
#[tauri::command]
pub fn config_form(text: String, section: Option<String>) -> FormView {
    form::form_view(&text, section.as_deref())
}

/// The text edits for one form change; an error message if it can't be made.
#[tauri::command]
pub fn config_form_edit(text: String, op: EditOp) -> Result<EditResult, String> {
    edit::apply(&text, &op)
}
