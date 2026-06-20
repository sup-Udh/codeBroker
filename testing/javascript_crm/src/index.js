const logger = require('./shared/logger');
const CustomerService = require('./customers/CustomerService');
const LeadService = require('./sales/LeadService');
const ReportService = require('./reports/ReportService');
const AuthService = require('./auth/AuthService');
const PaymentProcessor = require('./billing/PaymentProcessor');

logger.info('Starting CRM Benchmark App...');

try {
  // Test Auth
  const token = AuthService.login('admin', 'secret');
  
  // Test Customer
  CustomerService.createCustomer({ id: 'c1', name: 'Acme Corp', email: 'contact@acme.com' });
  
  // Test Lead & Sales (circular dependency execution)
  LeadService.processLead({ id: 'l1', name: 'John Doe', email: 'john@example.com' });
  
  // Test Report
  ReportService.sendWeeklyReport();

  // Test Billing
  PaymentProcessor.processPayment('inv-123');

  logger.info('All modules loaded and executed successfully.');
} catch (error) {
  logger.error(`Application crashed: ${error.message}`);
}
