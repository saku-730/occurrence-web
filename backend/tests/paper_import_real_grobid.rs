use backend::features::paper_import::grobid::{GrobidClient, PaperMetadataExtractor};

fn build_valid_pdf() -> Vec<u8> {
    let content = concat!(
        "BT\n",
        "/F1 20 Tf\n",
        "72 740 Td\n",
        "(A Study of Earthworm Distribution) Tj\n",
        "0 -30 Td\n",
        "/F1 12 Tf\n",
        "(Jane Doe and John Smith) Tj\n",
        "0 -24 Td\n",
        "(Example Journal 2026 Volume 12 Issue 3 Pages 101-115) Tj\n",
        "ET\n"
    );

    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        format!(
            "<< /Length {} >>\nstream\n{}endstream",
            content.as_bytes().len(),
            content
        ),
    ];

    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");

    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
    }

    let xref_offset = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            objects.len() + 1,
            xref_offset
        )
        .as_bytes(),
    );

    pdf
}

#[tokio::test]
#[ignore = "requires a real GROBID server at GROBID_BASE_URL"]
async fn real_grobid_extracts_header_from_valid_pdf() {
    let pdf_bytes = build_valid_pdf();
    let pdf = tempfile::Builder::new()
        .prefix("paper-import-real-grobid-")
        .suffix(".pdf")
        .tempfile()
        .expect("failed to create smoke-test PDF");
    std::fs::write(pdf.path(), &pdf_bytes).expect("failed to write smoke-test PDF");

    let client = GrobidClient::from_env().expect("real GROBID client should initialize");
    let metadata = client
        .extract_header(pdf.path(), pdf_bytes.len() as u64)
        .await
        .expect("real GROBID should parse the valid PDF");

    println!("real GROBID metadata: {metadata:#?}");

    assert!(
        metadata
            .title
            .as_deref()
            .is_some_and(|title| !title.trim().is_empty()),
        "real GROBID should extract a non-empty title from the PDF"
    );
}
