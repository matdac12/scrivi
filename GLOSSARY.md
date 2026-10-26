# scrivi

A minimal Windows text editor that replaces Notepad: the default for every plain-text file, with extra help for markdown.

## Language

**Text file**:
Any plain-text file scrivi opens, whatever its extension (or none).
_Avoid_: txt file, document

**Markdown file**:
A text file with a `.md` or `.markdown` extension; the only kind that gets markdown styling and a rendered view.
_Avoid_: md file, note

**Window**:
scrivi's single on-screen frame, which holds every open tab.
_Avoid_: instance, app

**Tab**:
One open text file, or one new unsaved one, inside the window.
_Avoid_: document, buffer, page

**Untitled tab**:
A tab whose text has never been saved to a file.
_Avoid_: new file, scratch

**Source view**:
The editable view of a file: its raw text, with light markdown styling for markdown files.
_Avoid_: editor mode, edit view

**Rendered view**:
A read-only view of a markdown file as formatted output, toggled from the source view.
_Avoid_: preview, reading mode

**Disk change**:
The file on disk changing under an open window, typically because an agent edited it.
_Avoid_: external edit, conflict
