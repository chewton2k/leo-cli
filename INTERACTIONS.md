# Stable editing and desktop updates

The desktop refuses foreground requests while its exclusive update installation
lease is held. An explicit update restart uses a private atomic one-use marker
to return the app to the foreground, including after a hidden login launch.
Installation failure removes the marker and releases the opening restriction.
Native installer replacement and OS activation still require release smoke tests;
Windows hands installation to an external NSIS process after Leo exits.

The note editor retains rendered list containers, numbers and task checkboxes
while editing their text. Markdown prefixes remain in the saved source and all
selection offsets account for them. On focus, the editor retains the rendered
text height and scrolls internally for longer inline Markdown; actual input
resumes normal height fitting. Heading prefixes hang left of their text.
Select-all and multi-line undo selections keep the document rendered. Replacing
a selection operates on its complete Markdown range, including list markers.

Application controls resist highlighting; readable content and text fields remain
selectable. Only explicit note/folder sources can start native drags. Link,
image and selection drags are suppressed; file drops and pointer-driven board
moves, resizing, whiteboards and graph interaction retain their existing paths.
