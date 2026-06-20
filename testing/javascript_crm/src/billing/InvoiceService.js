const logger = require('../shared/logger');
const EmailService = require('../notifications/EmailService');

class InvoiceService {
  generateInvoice(customerId, amount) {
    logger.info(`Generating invoice for customer ${customerId}: $${amount}`);
    EmailService.sendEmail('billing@crm.com', 'Invoice Generated', `Invoice for $${amount}`);
  }
}
module.exports = new InvoiceService();
