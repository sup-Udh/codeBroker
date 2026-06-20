const logger = require('../shared/logger');
const EmailService = require('../notifications/EmailService');
const SalesService = require('./SalesService');

class OpportunityService {
  closeOpportunity(opportunityId) {
    logger.info(`Closing opportunity ${opportunityId}`);
    EmailService.sendEmail('sales-manager@crm.com', 'Opportunity Closed', `ID: ${opportunityId}`);
  }
}
module.exports = new OpportunityService();
