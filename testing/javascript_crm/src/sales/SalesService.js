const logger = require('../shared/logger');
const EmailService = require('../notifications/EmailService');

class SalesService {
  createOpportunity(lead) {
    logger.info(`Creating opportunity for lead ${lead.id}`);
    EmailService.sendEmail('sales@crm.com', 'New Opportunity', `Opportunity generated for ${lead.name}`);
    
    // Circular dependency
    const LeadService = require('./LeadService');
    LeadService.updateLeadStatus(lead.id, 'QUALIFIED');
  }
}
module.exports = new SalesService();
