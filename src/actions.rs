//! All application commands. Keeping them in one place lets the menu bar,
//! the toolbar and the keyboard share the same action types (and therefore the
//! same shortcut hints in menus).

use gpui_kit::*;

actions!(
    inkdown,
    [
        NewDocument,
        OpenDocument,
        OpenFolder,
        RefreshFolder,
        NewFile,
        NewFolder,
        RenameFile,
        DeleteFile,
        SaveDocument,
        SaveDocumentAs,
        ExportHtml,
        ToggleTheme,
        ToggleSidebar,
        ViewWrite,
        ViewSplit,
        ViewPreview,
        FormatBold,
        FormatItalic,
        FormatStrike,
        FormatInlineCode,
        FormatCodeBlock,
        FormatLink,
        FormatHeading1,
        FormatHeading2,
        FormatQuote,
        FormatBulletList,
        FormatOrderedList,
        FormatTable,
        FormatDivider,
        PublishArticle,
        AiWrite,
        OpenSettings,
        CreateBackup,
        GitSync,
        RevealInFiles,
        CloseOverlay,
    ]
);
