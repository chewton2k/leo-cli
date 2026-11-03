# Stable editing and desktop updates

The desktop window must refuse foreground requests while an update is being
installed. An explicit restart after installation must return the updated app
to the foreground, including when it originally started hidden at login.
Installation failure must release the opening restriction and leave Leo usable.

The note editor will keep list markers, numbers and task checkboxes rendered
while editing their text. The editable text will occupy the same position and
use the same typography as the rendered line. Markdown remains the saved format;
selection, copy, cut, indentation and list continuation must preserve it.

The page will suppress selection on application controls and native dragging of
links and images, while retaining selection of readable content and text fields.
Explicit note/folder drags, board moves, file drops and resize handles remain
available. Existing editor, event and style modules own these changes; no new
editor dependency or alternate document model is needed.
