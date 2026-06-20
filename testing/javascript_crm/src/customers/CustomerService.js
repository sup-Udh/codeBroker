const logger = require('../shared/logger');
const EmailService = require('../notifications/EmailService');
const CustomerRepository = require('./CustomerRepository');
const CustomerValidator = require('./CustomerValidator');

class CustomerService {
  createCustomer(customerData) {
    logger.info('Creating customer...');
    if (CustomerValidator.validate(customerData)) {
      const saved = CustomerRepository.save(customerData);
      EmailService.sendEmail(saved.email, 'Welcome', 'Welcome to our CRM!');
      return saved;
    }
    throw new Error('Invalid customer data');
  }
}
module.exports = new CustomerService();
