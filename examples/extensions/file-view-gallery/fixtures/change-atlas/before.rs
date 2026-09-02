pub struct InvoiceLine {
    pub description: String,
    pub quantity: f64,
    pub unit_price: f64,
}

pub struct Invoice {
    pub id: String,
    pub customer_id: String,
    pub lines: Vec<InvoiceLine>,
    pub discount_percent: Option<f64>,
}

pub fn subtotal(invoice: &Invoice) -> f64 {
    invoice
        .lines
        .iter()
        .map(|line| line.quantity * line.unit_price)
        .sum()
}

pub fn discount(invoice: &Invoice) -> f64 {
    subtotal(invoice) * (invoice.discount_percent.unwrap_or(0.0) / 100.0)
}

pub fn total(invoice: &Invoice) -> f64 {
    subtotal(invoice) - discount(invoice)
}

pub fn format_invoice(invoice: &Invoice) -> String {
    let amount = total(invoice);
    format!("{}: ${amount:.2}", invoice.id)
}

pub fn can_send(invoice: &Invoice) -> bool {
    !invoice.lines.is_empty() && total(invoice) > 0.0
}

pub struct CustomerSummary {
    pub invoice_count: usize,
    pub total_revenue: f64,
}

pub fn summarize_customer(invoices: &[Invoice]) -> CustomerSummary {
    let total_revenue = invoices.iter().map(total).sum();
    CustomerSummary {
        invoice_count: invoices.len(),
        total_revenue,
    }
}
