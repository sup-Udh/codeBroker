const logger = require('../shared/logger');
const EmailService = require('../notifications/EmailService');
const InvoiceService = require('./InvoiceService');

class PaymentProcessor {
  processPayment(invoiceId) {
    logger.info(`Processing payment for invoice ${invoiceId}`);
    EmailService.sendEmail('billing@crm.com', 'Payment Processed', `Payment received for ${invoiceId}`);
  }
}
module.exports = new PaymentProcessor();
