//! nanoclean: remove o lixo que o nanoCAD deixa nos DWG, sem precisar de AutoCAD.
//!
//!   nanoclean <entrada.dwg> <saida.dwg> [--purge] [--keep-datalinks]
//!
//! O que faz (mesma limpeza do LimparNanoCAD.ps1):
//!   1. remove as tabelas de ordem de desenho duplicadas (*A1, *A2...) dos
//!      dicionarios de extensao de todos os blocos/espacos (ACAD_SORTENTS fica);
//!   2. elimina os blocos "Clipboard*" que nao estao em uso
//!      (com --purge: todos os blocos nomeados nao usados);
//!   3. remove os vinculos de tabela com Excel (DATALINK), porque a biblioteca
//!      de DWG ainda nao os grava corretamente e o AutoCAD recusaria o arquivo.
//!      As tabelas continuam no desenho, com o conteudo atual; so deixam de
//!      estar vinculadas a planilha;
//!   4. apaga hachuras sem nenhum contorno (o AUDIT do AutoCAD faria o mesmo).
//!
//! O progresso sai como uma linha JSON por evento no stdout.

use acadrust::entities::EntityType;
use acadrust::objects::ObjectType;
use acadrust::{DwgReader, DwgWriter, Handle};
use std::collections::HashSet;
use std::time::Instant;

struct Out {
    t0: Instant,
}
impl Out {
    fn emit(&self, stage: &str, fields: &[(&str, String)]) {
        let mut s = format!("{{\"stage\":\"{stage}\",\"secs\":{:.1}", self.t0.elapsed().as_secs_f64());
        for (k, v) in fields {
            s.push_str(&format!(",\"{k}\":{v}"));
        }
        s.push('}');
        println!("{s}");
    }
    fn fail(&self, msg: String) -> ! {
        self.emit("error", &[("error", json_str(&msg))]);
        std::process::exit(1);
    }
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// `*A1`, `*A2`... (mesmo criterio do wcmatch "`*A*" do script LISP)
fn is_anon_a(key: &str) -> bool {
    key.starts_with("*A")
}

fn is_datalink(o: &ObjectType) -> bool {
    matches!(o, ObjectType::ClassObject(c) if matches!(c.data, acadrust::objects::ClassObjectData::DataLink(_)))
}

/// Remove do mapa de objetos e de todos os dicionarios.
fn drop_objects(doc: &mut acadrust::CadDocument, hs: &HashSet<Handle>) -> usize {
    let mut n = 0;
    for h in hs {
        if doc.objects.remove(h).is_some() {
            n += 1;
        }
    }
    for o in doc.objects.values_mut() {
        if let ObjectType::Dictionary(d) = o {
            let before = d.entries.len();
            d.entries.retain(|(_, h)| !hs.contains(h));
            if d.entries.len() != before {
                let keys: HashSet<&str> = d.entries.iter().map(|(k, _)| k.as_str()).collect();
                d.hard_owner_entries.retain(|k| keys.contains(k.as_str()));
            }
        }
    }
    n
}

/// Nomes de blocos referenciados por algum INSERT (em qualquer espaco ou bloco).
fn referenced_blocks(doc: &acadrust::CadDocument) -> HashSet<String> {
    let mut s = HashSet::new();
    for e in doc.entities() {
        if let EntityType::Insert(i) = e {
            s.insert(i.block_name.to_uppercase());
        }
    }
    s
}

/// Apaga blocos nao usados cujo nome satisfaz `pick`. Repete ate estabilizar
/// (apagar um bloco pode liberar os blocos aninhados nele).
fn purge_blocks(doc: &mut acadrust::CadDocument, pick: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut gone = Vec::new();
    loop {
        let used = referenced_blocks(doc);
        let victims: Vec<(String, Handle)> = doc
            .block_records
            .iter()
            .filter(|b| {
                let up = b.name.to_uppercase();
                !b.name.starts_with('*') // espacos e blocos anonimos (cotas, dinamicos)
                    && !b.flags.is_xref
                    && !b.flags.is_xref_overlay
                    && !b.flags.anonymous
                    && pick(&b.name)
                    && !used.contains(&up)
            })
            .map(|b| (b.name.clone(), b.handle))
            .collect();
        if victims.is_empty() {
            break;
        }
        for (name, bh) in victims {
            let owned: Vec<Handle> = doc
                .entities()
                .filter(|e| e.common().owner_handle == bh)
                .map(|e| e.common().handle)
                .collect();
            for h in owned {
                doc.remove_entity(h);
            }
            if let Some(xd) = doc.extension_dictionary_handle(bh) {
                let mut hs = HashSet::new();
                hs.insert(xd);
                if let Some(ObjectType::Dictionary(d)) = doc.objects.get(&xd) {
                    hs.extend(d.entries.iter().map(|(_, h)| *h));
                }
                drop_objects(doc, &hs);
            }
            doc.block_records.remove(&name);
            gone.push(name);
        }
    }
    gone
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("uso: nanoclean <entrada.dwg> <saida.dwg> [--purge] [--keep-datalinks]");
        std::process::exit(2);
    }
    let (inp, outp) = (&args[1], &args[2]);
    let flag = |f: &str| args.iter().any(|a| a == f);
    let out = Out { t0: Instant::now() };

    // ---------- leitura ----------
    out.emit("load", &[]);
    let mut skipped: Vec<String> = Vec::new();
    let mut preserved: Vec<String> = Vec::new();
    let mut doc = match DwgReader::from_file(inp).and_then(|mut r| r.read()) {
        Ok(d) => d,
        Err(first) => {
            // registro corrompido: tenta de novo pulando o que nao for legivel
            out.emit("load", &[("failsafe", "true".into()), ("reason", json_str(&first.to_string()))]);
            let opts = acadrust::io::dwg::DwgReadOptions::failsafe();
            match DwgReader::from_file_with_options(inp, opts).and_then(|mut r| r.read()) {
                Ok(d) => {
                    for n in d.notifications.iter() {
                        if !matches!(n.notification_type, acadrust::notification::NotificationType::Error) {
                            continue;
                        }
                        // registros que nao decodificam mas foram mantidos byte a byte
                        // (vao para o DWG limpo intactos) nao sao perda
                        if n.message.starts_with("Preserved ") {
                            preserved.push(n.message.clone());
                        } else {
                            skipped.push(n.message.clone());
                        }
                    }
                    d
                }
                Err(e) => out.fail(format!("Não foi possível ler o DWG: {e}")),
            }
        }
    };
    if !preserved.is_empty() {
        let sample: Vec<String> = preserved.iter().take(5).map(|s| json_str(s)).collect();
        out.emit("preserved", &[("count", preserved.len().to_string()), ("messages", format!("[{}]", sample.join(",")))]);
    }
    if !skipped.is_empty() {
        let sample: Vec<String> = skipped.iter().take(20).map(|s| json_str(s)).collect();
        out.emit("skipped", &[("count", skipped.len().to_string()), ("messages", format!("[{}]", sample.join(",")))]);
    }
    let blocks_before = doc.block_records.len();
    out.emit("loaded", &[
        ("version", json_str(&format!("{:?}", doc.version))),
        ("entities", doc.entities().count().to_string()),
        ("objects", doc.objects.len().to_string()),
        ("blocks", blocks_before.to_string()),
    ]);

    // ---------- 1. tabelas *A ----------
    let owners: Vec<Handle> = doc.block_records.iter().map(|b| b.handle).collect();
    let mut drop: HashSet<Handle> = HashSet::new();
    let mut total_anon = 0usize;
    for owner in owners {
        let Some(xd) = doc.extension_dictionary_handle(owner) else { continue };
        let Some(ObjectType::Dictionary(dict)) = doc.objects.get(&xd) else { continue };
        for (k, h) in &dict.entries {
            if is_anon_a(k) {
                total_anon += 1;
                if matches!(doc.objects.get(h), Some(ObjectType::SortEntitiesTable(_))) {
                    drop.insert(*h);
                }
            }
        }
    }
    out.emit("erase", &[("done", "0".into()), ("total", drop.len().to_string())]);
    let removed = drop_objects(&mut doc, &drop);
    let left = total_anon - removed;
    out.emit("erase", &[("done", removed.to_string()), ("total", drop.len().to_string())]);

    // ---------- 2. blocos Clipboard / purge ----------
    out.emit("purge", &[]);
    let purge_all = flag("--purge");
    let purged = purge_blocks(&mut doc, &|name: &str| {
        purge_all || name.to_uppercase().starts_with("CLIPBOARD")
    });

    // ---------- 3. DATALINK ----------
    let mut datalinks = 0;
    if !flag("--keep-datalinks") {
        let dl: HashSet<Handle> = doc.objects.iter().filter(|(_, o)| is_datalink(o)).map(|(h, _)| *h).collect();
        datalinks = drop_objects(&mut doc, &dl);
    }

    // ---------- 4. hachuras sem nenhum contorno (o AUDIT do AutoCAD apagaria) ----------
    let bad_hatches: Vec<Handle> = doc
        .entities()
        .filter(|e| matches!(e, EntityType::Hatch(h) if h.paths.is_empty()))
        .map(|e| e.common().handle)
        .collect();
    for h in &bad_hatches {
        doc.remove_entity(*h);
    }

    // ---------- gravacao ----------
    out.emit("save", &[]);
    let res = if outp.to_lowercase().ends_with(".dxf") {
        acadrust::DxfWriter::new(&doc).write_to_file(outp)
    } else {
        DwgWriter::write_to_file(outp, &doc)
    };
    if let Err(e) = res {
        out.fail(format!("Não foi possível gravar o DWG: {e}"));
    }
    out.emit("done", &[
        ("removed", removed.to_string()),
        ("left", left.to_string()),
        ("blocks_purged", purged.len().to_string()),
        ("datalinks_removed", datalinks.to_string()),
        ("invalid_hatches_removed", bad_hatches.len().to_string()),
        ("preserved_raw", preserved.len().to_string()),
        ("version", json_str(&format!("{:?}", doc.version))),
    ]);
}
