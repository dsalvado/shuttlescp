# Contributing to ShuttleSCP

Thank you for helping. Bug reports and ideas are welcome as
[issues](https://github.com/dsalvado/shuttlescp/issues); security problems please report
privately, as described in [SECURITY.md](SECURITY.md).

## Translations

ShuttleSCP's interface texts live in [Fluent](https://projectfluent.org) files, one
folder per language:

```
crates/shuttlescp-gtk/i18n/en-US/shuttlescp.ftl   English, the original
crates/shuttlescp-gtk/i18n/fr-FR/shuttlescp.ftl   French
crates/shuttlescp-gtk/i18n/es-ES/shuttlescp.ftl   Spanish
crates/shuttlescp-gtk/i18n/de-DE/shuttlescp.ftl   German
crates/shuttlescp-gtk/i18n/pt-BR/shuttlescp.ftl   Brazilian Portuguese
crates/shuttlescp-gtk/i18n/zh-CN/shuttlescp.ftl   Simplified Chinese
```

The translations were made with AI and have not been reviewed by native speakers yet.
If you speak one of these languages, corrections are the most useful help there is.

### Correcting a translation

Each line is a message: a name, `=`, and the text. Change only the text on the right:

```
pane-items =
    { $count ->
        [one] { $count } élément
       *[other] { $count } éléments
    }
```

- **Keep every `{ $variable }`.** The program fills them in (a file name, a number, a
  server's address). You may move them to suit your language's word order, but not drop
  or rename them.
- **`{ -app-name }`** is the product's name; leave it as it is.
- **An underscore** (`_Fichier`) marks the letter for the keyboard shortcut (Alt+F). Keep
  one per menu item, on letters that differ within each menu.
- **Plurals** use Fluent selectors, as above: write the variants your language needs
  (`[one]`, `[few]`, `[many]`, `*[other]`, ...). See the
  [Fluent guide](https://projectfluent.org/fluent/guide/selectors.html).
- The comments (lines starting with `#`) in the English file say what each variable
  holds.

You can edit the file right on GitHub (the pencil icon), which opens a pull request.
To see your changes in the app, build it as described in [BUILDING.md](BUILDING.md),
pick the language in File › Preferences and restart.

Once a language has been reviewed by a native speaker, its "(machine translation)" label
in `language-name` at the top of the file can go.

### Adding a language

Copy `i18n/en-US/shuttlescp.ftl` to a new folder named after the language's tag (for
example `it-IT` or `ja-JP`), translate the texts, and set `language-name` to the
language's own name. Nothing else needs changing: the language list is built from the
folders.

### How translations are checked

Every pull request runs automatic checks, which fail if a translation:

- lacks a message the English file has, or has one it does not,
- drops a `{ $variable }`, or uses one the English message does not have,
- adds a web address the English text does not have,
- cannot be read (a syntax error).

Then each change is reviewed before it is merged. The maintainer does not read every
language, so the review includes a back-translation: each changed text is translated
back into English, with AI, and compared with the original, to catch changed meanings
and anything inappropriate. Please keep the texts plain and close to the English
meaning; jokes and slang will not be accepted.
