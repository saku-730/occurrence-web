use anyhow::{Context, Result};
use csv::ReaderBuilder;
use flate2::read::GzDecoder;
use std::env;
use std::fs::File;
use std::io::{BufWriter, Write};

const GRAPH: &str = "https://bio-database.net/graphs/taxonomy/gbif-backbone";
const TAXON_BASE: &str = "https://bio-database.net/taxa/gbif/";

const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
const DWC_TAXON: &str = "http://rs.tdwg.org/dwc/terms/Taxon";
const DWC_TAXON_ID: &str = "http://rs.tdwg.org/dwc/terms/taxonID";
const DWC_SCIENTIFIC_NAME: &str = "http://rs.tdwg.org/dwc/terms/scientificName";
const DWC_TAXON_RANK: &str = "http://rs.tdwg.org/dwc/terms/taxonRank";
const DWC_TAXONOMIC_STATUS: &str = "http://rs.tdwg.org/dwc/terms/taxonomicStatus";

const EX_PARENT_NAME_USAGE: &str = "https://bio-database.net/terms/parentNameUsage";
const EX_BASIONYM: &str = "https://bio-database.net/terms/basionym";
const EX_IS_SYNONYM: &str = "https://bio-database.net/terms/isSynonym";
const EX_CANONICAL_NAME: &str = "https://bio-database.net/terms/canonicalName";
const EX_AUTHORSHIP: &str = "https://bio-database.net/terms/authorship";
const EX_GBIF_ISSUE: &str = "https://bio-database.net/terms/gbifIssue";

const EX_KINGDOM_TAXON: &str = "https://bio-database.net/terms/kingdomTaxon";
const EX_PHYLUM_TAXON: &str = "https://bio-database.net/terms/phylumTaxon";
const EX_CLASS_TAXON: &str = "https://bio-database.net/terms/classTaxon";
const EX_ORDER_TAXON: &str = "https://bio-database.net/terms/orderTaxon";
const EX_FAMILY_TAXON: &str = "https://bio-database.net/terms/familyTaxon";
const EX_GENUS_TAXON: &str = "https://bio-database.net/terms/genusTaxon";
const EX_SPECIES_TAXON: &str = "https://bio-database.net/terms/speciesTaxon";

fn taxon_uri(id: &str) -> String {
    format!("{TAXON_BASE}{id}")
}

fn is_blank(s: &str) -> bool {
    s.trim().is_empty() || s == "\\N"
}

fn escape_literal(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect::<Vec<_>>(),
            '\r' => "\\r".chars().collect::<Vec<_>>(),
            '\t' => "\\t".chars().collect::<Vec<_>>(),
            _ => vec![c],
        })
        .collect()
}

fn write_uri_triple<W: Write>(w: &mut W, s: &str, p: &str, o: &str) -> Result<()> {
    writeln!(w, "<{s}> <{p}> <{o}> <{GRAPH}> .")?;
    Ok(())
}

fn write_literal_triple<W: Write>(w: &mut W, s: &str, p: &str, o: &str) -> Result<()> {
    if is_blank(o) {
        return Ok(());
    }

    let escaped = escape_literal(o);
    writeln!(w, "<{s}> <{p}> \"{escaped}\" <{GRAPH}> .")?;
    Ok(())
}

fn write_taxon_ref<W: Write>(w: &mut W, s: &str, p: &str, id: &str) -> Result<()> {
    if is_blank(id) {
        return Ok(());
    }

    let object = taxon_uri(id);
    write_uri_triple(w, s, p, &object)
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect(); 

    if args.len() < 3 {
        eprintln!(
            "Usage: {} <input simple.txt.gz> <output gbif-backbone.nq> [limit]",
            args[0] //arg[0] this file name
        );
        std::process::exit(1);
    }

    let input_path = &args[1];
    let output_path = &args[2];

    let limit = args
        .get(3)
        .map(|s| s.parse::<usize>())
        .transpose()
        .context("limit must be a number")?;

    let input = File::open(input_path)
        .with_context(|| format!("failed to open input file: {input_path}"))?;
    let gz = GzDecoder::new(input);

    let mut reader = ReaderBuilder::new()
        .delimiter(b'\t')
        .has_headers(false)
        .flexible(true)
        .from_reader(gz);

    let output = File::create(output_path)
        .with_context(|| format!("failed to create output file: {output_path}"))?;
    let mut writer = BufWriter::new(output);

    for (i, result) in reader.records().enumerate() {//iは行番号
        if let Some(limit) = limit {
            if i >= limit {
                break;
            }
        }

        let record = result.with_context(|| format!("failed to read record at line {}", i + 1))?;

        // simple.txt.gz の列順。READMEのSELECT順に合わせる。
        let id = record.get(0).unwrap_or("");
        let parent_fk = record.get(1).unwrap_or("");
        let basionym_fk = record.get(2).unwrap_or("");
        let is_synonym = record.get(3).unwrap_or("");
        let status = record.get(4).unwrap_or("");
        let rank = record.get(5).unwrap_or("");

        let kingdom_fk = record.get(10).unwrap_or("");
        let phylum_fk = record.get(11).unwrap_or("");
        let class_fk = record.get(12).unwrap_or("");
        let order_fk = record.get(13).unwrap_or("");
        let family_fk = record.get(14).unwrap_or("");
        let genus_fk = record.get(15).unwrap_or("");
        let species_fk = record.get(16).unwrap_or("");

        let scientific_name = record.get(18).unwrap_or("");
        let canonical_name = record.get(19).unwrap_or("");
        let authorship = record.get(24).unwrap_or("");
        let issues = record.get(29).unwrap_or("");

        if is_blank(id) { //id なしはグラフ作らず破棄
            continue;
        }

        let subject = taxon_uri(id);

        write_uri_triple(&mut writer, &subject, RDF_TYPE, DWC_TAXON)?;
        write_literal_triple(&mut writer, &subject, DWC_TAXON_ID, id)?;
        write_literal_triple(&mut writer, &subject, DWC_SCIENTIFIC_NAME, scientific_name)?;
        write_literal_triple(&mut writer, &subject, EX_CANONICAL_NAME, canonical_name)?;
        write_literal_triple(&mut writer, &subject, DWC_TAXON_RANK, rank)?;
        write_literal_triple(&mut writer, &subject, DWC_TAXONOMIC_STATUS, status)?;
        write_literal_triple(&mut writer, &subject, EX_IS_SYNONYM, is_synonym)?;
        write_literal_triple(&mut writer, &subject, EX_AUTHORSHIP, authorship)?;
        write_literal_triple(&mut writer, &subject, EX_GBIF_ISSUE, issues)?;

        write_taxon_ref(&mut writer, &subject, EX_PARENT_NAME_USAGE, parent_fk)?;
        write_taxon_ref(&mut writer, &subject, EX_BASIONYM, basionym_fk)?;

        write_taxon_ref(&mut writer, &subject, EX_KINGDOM_TAXON, kingdom_fk)?;
        write_taxon_ref(&mut writer, &subject, EX_PHYLUM_TAXON, phylum_fk)?;
        write_taxon_ref(&mut writer, &subject, EX_CLASS_TAXON, class_fk)?;
        write_taxon_ref(&mut writer, &subject, EX_ORDER_TAXON, order_fk)?;
        write_taxon_ref(&mut writer, &subject, EX_FAMILY_TAXON, family_fk)?;
        write_taxon_ref(&mut writer, &subject, EX_GENUS_TAXON, genus_fk)?;
        write_taxon_ref(&mut writer, &subject, EX_SPECIES_TAXON, species_fk)?;
    }

    writer.flush()?;
    Ok(())
}