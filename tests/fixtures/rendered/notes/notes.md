# Note di lavoro — ottobre

Un posto unico per appunti personali e di sviluppo. Mezzo italiano, mezzo English, come sempre. Jump to [the table](#tools), or read about [WebView2](https://learn.microsoft.com/microsoft-edge/webview2/) and [write to me](mailto:someone@example.com).

## Oggi

- [x] Rispondere a Giulia sul preventivo
- [x] Fix the flaky test in `sync_worker`
- [ ] Prenotare il tagliando della macchina
- [ ] Review the PR for the **export** feature
  - [ ] check the CSV escaping
  - [ ] confirm *date formats* with the team

## Idee per scrivi

Scrivi deve aprire qualsiasi file di testo in meno di un attimo. If it feels slower than Notepad, it has ~~succeeded~~ failed. Niente impostazioni, niente plugin: solo testo, tab e un tasto per leggere.

> La semplicità è la sofisticazione suprema.

### Comandi utili

```
cargo run --release -- notes.md
```

### Tools

| Tool | Stack | Idle |
|:-----|:------|-----:|
| tasca | Rust · Win32 | 4 MB |
| scrivi | Rust · Win32 | 30 MB |

![The scrivi window](img/window.svg)

A mark from the folder next door: ![mark](../shared/mark.svg)

<script>document.body.innerHTML = "scripts ran"</script>
