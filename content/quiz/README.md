# Quiz questions

The quiz event of the server (a GM starts it with `/qeo`) asks questions that **you** supply: the shipped database has
no questions, and the quiz refuses to open until you imported some (Plan/22 section 15, decision PD-12).

`example.toml` in this folder is a complete example file (8 four-choice and 6 true/false questions about Madrigal; the
last one is switched off to show `enabled = false`). Copy it and write your own.

## The file

A TOML file with one `[[question]]` table per question and nothing else:

```toml
[[question]]
type = "4c"                  # "4c" = four choices, "ox" = true or false (optional when you name --type)
question = "Which town is the starting point of every new character?"
choices = ["Saint Morning", "Flaris", "Azria", "Coral Island"]    # exactly 4 for "4c", leave out for "ox"
answer = 2                   # "4c": 1..4, "ox": 1 = O (true), 2 = X (false)
enabled = true               # optional, default true; a disabled question stays stored but is never asked

[[question]]
type = "ox"
question = "Penya is the currency of Madrigal.|True or false?"     # "|" starts a new line on the players' screen
answer = 1
```

* The question is at most 1024 bytes; question and choices together (one per line) are at most 1024 bytes when they
  are sent to the players. A choice is one non-empty line.
* A question is identified by its type and its text. The same text twice in one file is an error.
* Unknown keys and unknown types are errors. Every problem of the file is listed with its line, and nothing is
  imported unless the whole file is valid.

## The command

```
flyff quiz import <FILE> [--type N] [--replace]
```

`--home <dir>` (before or after the command) selects the server home, as for every `flyff` command.

| form | effect |
|---|---|
| `flyff quiz import questions.toml` | imports every entry; each entry names its `type` |
| `flyff quiz import questions.toml --type 2` | imports only the four-choice entries (`1` = OX, `2` = four choices); an entry may leave out `type` then; entries of the other type are skipped and counted |
| `flyff quiz import questions.toml --type 2 --replace` | also deletes the stored four-choice questions that are not in the file (the other type is never touched; the questions that stay keep their identity and their "last asked" time) |

Example output: `quiz import: questions.toml: 8 question(s), 6 skipped (other type): inserted 8, updated 0, unchanged 0, pruned 0`.

* **Idempotent.** Importing the same file again inserts nothing (`unchanged 8`). A changed answer, choice list or
  `enabled` flag is an update of the stored question; the text identifies it.
* **One transaction.** A file with a mistake reports `error: <FILE>:<line>: <message>` for every problem, exits with
  status 1 and changes nothing.
* **While the server runs.** The import uses its own short database connection and does not need the server stopped;
  if the server is writing at that moment it waits briefly, then reports that the database is busy (run it again).
  The database must already exist (`flyff db migrate`, or start the server once).
* The import is audited (`quiz.import` in `logs/audit/`).

## Which questions are asked

The quiz type is the one of `QuizEvent.lua` (`QuizType`, four choices in the shipped script; an
`overrides/QuizEvent.lua` with `QuizType = TYPE_OX` runs a true/false quiz and needs OX questions, `--type 1`). A quiz
asks the enabled questions of that type, the ones that were never asked first and then the ones asked longest ago, so
several quizzes in a row do not repeat themselves until every question was asked. The questions of a quiz are marked
"asked" when their round ends.
