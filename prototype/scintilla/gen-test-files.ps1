# Generates the prototype's test files under %TEMP%\scrivi-proto-files\.
# Never writes into the repo. Usage: powershell -File gen-test-files.ps1
$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:TEMP 'scrivi-proto-files'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$utf8 = New-Object System.Text.UTF8Encoding($false)

$notes = @'
# Note di lavoro — ottobre

Un posto unico per appunti personali e di sviluppo. Mezzo italiano, mezzo English, come sempre.

## Oggi

- [x] Rispondere a Giulia sul preventivo
- [x] Fix the flaky test in `sync_worker`
- [ ] Prenotare il tagliando della macchina
- [ ] Review the PR for the **export** feature
  - [ ] check the CSV escaping
  - [ ] confirm _date formats_ with the team
- [ ] Comprare il pane (e il caffè, **soprattutto** il caffè)

## Idee per scrivi

Scrivi deve aprire qualsiasi file di testo in meno di un attimo. If it feels slower than Notepad, it has failed.

1. Apertura istantanea, anche da Explorer con dieci file selezionati
2. Markdown styling leggero: headings in **bold**, markers grey but visible
3. Rendered view on `Ctrl+E`
   1. read-only
   2. selezionabile e copiabile
   3. no scripts, ever
4. Tabs, but only if they stay out of the way

> La semplicità è la sofisticazione suprema.
> Simplicity is not the absence of features, it is the absence of friction.

### Domande aperte

- Does the line-number margin earn its place, or is it programmer noise?
- Quanto deve essere grande il caret? 1 px sembra fragile su schermi 4K.
- ~~Use RichEdit for everything~~ — no, too slow on big files.
- What happens when an agent rewrites the file while I am typing? See [disk change](https://example.com/scrivi/disk-change) notes.

## Meeting — progetto Atlante

Partecipanti: Marco, Sara, io. Durata: 45 minuti, che sono diventati 70.

### Decisioni

- Il rilascio slitta di una settimana, **non di più**.
- The API stays on `v2`; no breaking changes until January.
- Sara owns the migration script; Marco reviews.

### Action items

| Chi   | Cosa                              | Entro      | Stato        |
|-------|-----------------------------------|------------|--------------|
| Sara  | Migration script for `orders`     | 31/10      | in corso     |
| Marco | Load test with 10k users          | 04/11      | da iniziare  |
| Io    | Aggiornare la documentazione      | 30/10      | **fatto**    |
| Tutti | Retro on the incident of last week | 06/11     | da fissare   |

### Note sparse

The incident was boring in the best way: a missing index on `orders.customer_id` made the nightly report scan forty million rows. Abbiamo aggiunto l'indice, il report è passato da 41 minuti a 52 secondi, e nessuno se n'è accorto tranne il database. Lesson: the slow query log was telling us for weeks, and nobody reads the slow query log, so the real action item is to page on it rather than to promise to read it more carefully next time, which we all know we will not do.

## Snippets

Quick way to find the biggest files in a repo:

```powershell
Get-ChildItem -Recurse -File |
    Sort-Object Length -Descending |
    Select-Object -First 20 FullName, @{n='MB';e={[math]::Round($_.Length / 1MB, 1)}}
```

Rust: read a file in chunks without holding it all in memory.

```rust
use std::io::Read;

fn count_lines(path: &str) -> std::io::Result<usize> {
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 1 << 20];
    let mut lines = 0;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 { break; }
        lines += buf[..n].iter().filter(|&&b| b == b'\n').count();
    }
    Ok(lines)
}
```

SQL che uso sempre per controllare i lock:

```sql
SELECT pid, state, wait_event_type, query
FROM pg_stat_activity
WHERE state <> 'idle'
ORDER BY query_start;
```

Inline: run `cargo build --release`, then `.\target\release\app.exe --help`. Remember `git switch -c` instead of `git checkout -b`.

## Lettura

- *The Design of Everyday Things* — rileggere il capitolo sui *signifiers*
- **Working in Public**, Nadia Eghbal: open source maintainers as a creator economy
- [Scintilla documentation](https://www.scintilla.org/ScintillaDoc.html), the section on styling
- _Il nome della rosa_, per la terza volta, perché sì

### Citazioni

> "Make it work, make it right, make it fast."
> — Kent Beck
>
> Nell'ordine. Il terzo passo di solito non serve.

## Viaggio a Lisbona

### Da fare prima di partire

- [x] Passaporto (scade a marzo, ok)
- [x] Prenotare l'alloggio ad Alfama
- [ ] Adattatore? No, Portogallo usa la stessa presa
- [ ] Download offline maps
- [ ] Lista ristoranti
  - Taberna da Rua das Flores
  - *Cervejaria Ramiro* per i frutti di mare
  - something vegetarian for Anna

### Budget

| Voce      | Stima   |
|-----------|--------:|
| Voli      | 180 €   |
| Alloggio  | 420 €   |
| Cibo      | 300 €   |
| Extra     | 150 €   |
| **Totale**| **1050 €** |

## Diario

### Lunedì

Giornata lenta. Ho passato la mattina a capire perché il build su CI fosse rosso: era un certificato scaduto nel runner, non il nostro codice. *Classic.* In the afternoon I finally wrote the parser for the config files, and it is half the size of the old one because it does not try to be clever about comments.

### Martedì

- Standup: niente di nuovo
- Pair programming con Luca sul modulo dei pagamenti
- **Bug trovato**: arrotondamento sbagliato sulle valute con tre decimali (dinaro kuwaitiano!)
- Evening: corsa di 8 km, piano

### Mercoledì

Note to self: when a test only fails on Wednesdays, check the date logic first. It was `weekday() == 2` hard-coded in a fixture. Nessun commento.

### Giovedì

Ho provato tre editor markdown diversi per una settimana:

1. Il primo era bellissimo ma impiegava due secondi ad aprirsi.
2. Il secondo era velocissimo ma brutto come un terminale del 1995.
3. Il terzo mangiava 400 MB di RAM per una lista della spesa.

Conclusione: scrivere il mio. Obviously.

## Checklist di rilascio

- [ ] Bump version in `Cargo.toml`
- [ ] Update `CHANGELOG.md`
- [ ] Tag: `git tag -a v0.3.0 -m "v0.3.0"`
- [ ] Build release, check the exe size is **under 2 MB**
- [ ] Smoke test:
  - [ ] open a `.txt` from Explorer
  - [ ] open a `.md`, toggle the rendered view
  - [ ] open a 50 MB log, scroll to the end
  - [ ] close with unsaved changes, check the prompt
- [ ] Publish the release notes

## Ricette

### Pasta al pomodoro (quella vera)

Ingredienti per due:

- 200 g di spaghetti
- 400 g di pomodori pelati, *buoni*
- uno spicchio d'aglio
- basilico fresco
- olio extravergine, sale

Procedimento: l'aglio va in camicia nell'olio a fuoco basso, poi tolto. Pomodoro, sale, venti minuti. Basilico solo alla fine, a fuoco spento. **Mai** la cipolla e l'aglio insieme: è una guerra di religione e io ho scelto la mia parte.

## Glossario veloce

Source view
: the editable text, with light styling

Rendered view
: read-only, formatted output

Disk change
: the file changed under us, usually because an agent edited it

## Long paragraph for wrapping

This paragraph is deliberately long so that soft wrap has something to do, and so we can see whether wrapped lines keep a sensible indent and whether the caret-line highlight covers the whole logical line or only the visual sub-line, which matters more than it sounds because a writer moves through prose by sentence and not by visual row. Questo paragrafo è volutamente lungo, così il ritorno a capo automatico ha qualcosa da fare e possiamo vedere se le righe spezzate mantengono un rientro sensato, e se l'evidenziazione della riga del cursore copre tutta la riga logica o soltanto la riga visiva, cosa che conta più di quanto sembri. A good editor gets out of the way: no gutter icons nobody asked for, no minimap, no breadcrumbs, just text, a quiet margin and a caret that is easy to find.

## Setup nuovo laptop

### Software

- [x] Windows Terminal, con il profilo *Cascadia Mono 11*
- [x] Git, configurato con `core.autocrlf=false`
- [ ] Rust toolchain
  - [x] `rustup` stable
  - [ ] `cargo-binstall`
  - [ ] `cargo-nextest`
- [ ] Node LTS via `fnm`
- [ ] Ripristinare le chiavi SSH dal backup

### Impostazioni da non dimenticare

1. Disattivare *Fast Startup* (fa danni con il dual boot)
2. Explorer: mostrare le estensioni dei file, **sempre**
3. Power plan: *Balanced*, ma con il coperchio chiuso → sospensione
4. Tastiera: layout italiano, ma con `AltGr` mappato come su US-International

> Nota: il BIOS si aggiorna **prima** di installare tutto il resto, non dopo.

## Appunti Rust

### Lifetimes, versione breve

- A reference cannot outlive the data it points to.
- `'static` means *can* live forever, not *does* live forever.
- When the compiler complains, the fix is usually to own the data (`String` instead of `&str`) and move on.

### Error handling

Per i tool piccoli: `anyhow` nel binario, `thiserror` nelle librerie. In a throwaway prototype, `unwrap()` is fine and honest.

```rust
#[derive(Debug, thiserror::Error)]
enum LoadError {
    #[error("file not found: {0}")]
    NotFound(String),
    #[error("not valid UTF-8 at byte {0}")]
    Encoding(usize),
}
```

### Cose da ricordare

- `cargo tree -d` mostra le dipendenze duplicate
- `cargo bloat --release --crates` per capire cosa gonfia l'exe
- ~~`opt-level = "z"`~~ è quasi sempre peggio di `"s"` per la velocità
- `strip = true` toglie i simboli: l'exe passa da 4 MB a 1 MB

## Spese ottobre

| Data  | Descrizione            | Categoria   | Importo |
|-------|------------------------|-------------|--------:|
| 02/10 | Spesa settimanale      | Casa        | 86,40 € |
| 05/10 | Benzina                | Auto        | 72,00 € |
| 09/10 | Cena con Marco e Sara  | Svago       | 54,00 € |
| 12/10 | Abbonamento palestra   | Salute      | 39,00 € |
| 18/10 | Libri                  | Cultura     | 31,50 € |
| 23/10 | Bolletta luce          | Casa        | 64,20 € |

Totale finora: **347,10 €**. Under budget, *per una volta*.

## Workout

- Lunedì: corsa 8 km
- Mercoledì: forza
  - squat 4×6
  - panca 4×6
  - trazioni 3×max
- Venerdì: corsa lenta 10 km
- Domenica: camminata in collina, se non piove

## Varie

- Telefonare alla nonna domenica
- Regalo per Paolo: forse un libro di fotografia?
- Rinnovare il dominio prima del **15 novembre**
- Backup del NAS: controllare che il job notturno giri davvero
- Learn enough `nushell` to decide whether it replaces PowerShell for me (spoiler: probably not)

---

*Ultimo aggiornamento: domenica sera, con il tè.*
'@

$notes = $notes -replace "`r`n", "`n"
[IO.File]::WriteAllText((Join-Path $dir 'notes.md'), $notes, $utf8)

# big.md: ~5 MB of repeated notes, each copy numbered so headings differ.
$sb = New-Object System.Text.StringBuilder
$i = 0
while ($sb.Length -lt 5MB) {
    $i++
    [void]$sb.Append("# Sezione $i`n`n").Append($notes).Append("`n`n")
}
[IO.File]::WriteAllText((Join-Path $dir 'big.md'), $sb.ToString(), $utf8)

# Log files: realistic service log lines with increasing timestamps.
function Write-Log([string]$path, [long]$bytes) {
    $levels = 'INFO', 'INFO', 'INFO', 'DEBUG', 'DEBUG', 'WARN', 'ERROR'
    $svcs = 'api.gateway', 'orders.worker', 'auth', 'billing.sync', 'search.indexer', 'mailer'
    $msgs = @(
        'request completed method=GET path=/v2/orders/{0} status=200 duration_ms={1}',
        'request completed method=POST path=/v2/checkout status=201 duration_ms={1} user_id={0}',
        'cache miss key=customer:{0} ttl=300 fallback=db duration_ms={1}',
        'retrying upstream call attempt=2 of 5 target=payments.internal:8443 order_id={0} backoff_ms={1}',
        'job finished name=nightly-report rows={0} duration_ms={1}',
        'token refreshed client_id=web-{0} expires_in=3600 latency_ms={1}',
        'slow query detected table=orders rows_scanned={0} duration_ms={1} query="SELECT * FROM orders WHERE customer_id = $1"',
        'connection pool stats active={1} idle=12 waiting=0 max=64 shard={0}',
        'email queued template=invoice recipient_hash=a3f9{0} size_bytes={1}',
        'index batch committed docs={0} segments=7 merge_ms={1}'
    )
    $w = New-Object System.IO.StreamWriter($path, $false, $utf8, 1MB)
    $rand = New-Object System.Random(42)
    $t = [datetime]'2026-10-01T00:00:00'
    $written = 0L
    $sbl = New-Object System.Text.StringBuilder
    while ($written -lt $bytes) {
        [void]$sbl.Clear()
        for ($k = 0; $k -lt 2000; $k++) {
            $t = $t.AddMilliseconds($rand.Next(1, 40))
            $lvl = $levels[$rand.Next($levels.Length)]
            $svc = $svcs[$rand.Next($svcs.Length)]
            $m = [string]::Format($msgs[$rand.Next($msgs.Length)], $rand.Next(100000, 999999), $rand.Next(1, 2500))
            [void]$sbl.Append($t.ToString('yyyy-MM-ddTHH:mm:ss.fffZ')).Append(' ').Append($lvl.PadRight(5)).Append(' [').Append($svc).Append('] trace=').Append($rand.Next().ToString('x8')).Append(' ').Append($m).Append("`n")
        }
        $s = $sbl.ToString()
        $w.Write($s)
        $written += $s.Length
    }
    $w.Close()
}
Write-Log (Join-Path $dir 'log50.log') 50000000
Write-Log (Join-Path $dir 'log200.log') 200000000

Get-ChildItem $dir | Select-Object Name, Length | Format-Table -AutoSize
