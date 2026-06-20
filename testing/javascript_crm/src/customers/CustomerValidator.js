const logger = require('../shared/logger');
const EmailService = require('../notifications/EmailService');

class CustomerValidator {
  validate(customer) {
    logger.info(`Validating customer ${customer.name}`);
    if (!customer.email) {
      EmailService.sendEmail('admin@crm.com', 'Validation Failed', 'Customer missing email');
      return false;
    }
    return true;
  }
}
module.exports = new CustomerValidator();
