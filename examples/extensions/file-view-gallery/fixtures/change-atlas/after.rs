pub struct InvoiceLine {
    pub description: String,
    pub quantity: f64,
    pub unit_price: f64,
    pub taxable: Option<bool>,
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
    let percent = invoice.discount_percent.unwrap_or(0.0).clamp(0.0, 30.0);
    subtotal(invoice) * (percent / 100.0)
}

pub fn total(invoice: &Invoice) -> f64 {
    let discounted = subtotal(invoice) - discount(invoice);
    let tax = invoice
        .lines
        .iter()
        .filter(|line| line.taxable != Some(false))
        .map(|line| line.quantity * line.unit_price * 0.08)
        .sum::<f64>();
    discounted + tax
}

pub fn format_invoice(invoice: &Invoice) -> String {
    let amount = total(invoice);
    let customer = format!("{:0>8}", invoice.customer_id);
    format!("{} · customer {customer} · ${amount:.2}", invoice.id)
}

pub fn can_send(invoice: &Invoice) -> bool {
    !invoice.lines.is_empty() && total(invoice) > 0.0
}

pub struct CustomerSummary {
    pub invoice_count: usize,
    pub total_revenue: f64,
    pub average_invoice: f64,
}

pub fn summarize_customer(invoices: &[Invoice]) -> CustomerSummary {
    let total_revenue = invoices.iter().map(total).sum::<f64>();
    let average_invoice = if invoices.is_empty() {
        0.0
    } else {
        total_revenue / invoices.len() as f64
    };
    CustomerSummary {
        invoice_count: invoices.len(),
        total_revenue,
        average_invoice,
    }
}
