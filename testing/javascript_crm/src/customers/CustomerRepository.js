const logger = require('../shared/logger');
const config = require('../shared/config');

class CustomerRepository {
  constructor() {
    this.customers = [];
    logger.info(`CustomerRepository initialized with DB: ${config.db.host}`);
  }
  save(customer) {
    logger.info(`Saving customer ${customer.name}`);
    this.customers.push(customer);
    return customer;
  }
  findById(id) {
    logger.info(`Finding customer by ID: ${id}`);
    return this.customers.find(c => c.id === id);
  }
}
module.exports = new CustomerRepository();
