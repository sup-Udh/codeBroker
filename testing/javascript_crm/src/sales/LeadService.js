const logger = require('../shared/logger');
const EmailService = require('../notifications/EmailService');

class LeadService {
  processLead(lead) {
    logger.info(`Processing lead ${lead.id}`);
    EmailService.sendEmail(lead.email, 'Lead Received', 'We have received your details.');
    
    // Circular dependency
    const SalesService = require('./SalesService');
    SalesService.createOpportunity(lead);
  }
  
  updateLeadStatus(leadId, status) {
    logger.info(`Lead ${leadId} status updated to ${status}`);
    EmailService.sendEmail('admin@crm.com', 'Lead Status Changed', `Lead ${leadId} is now ${status}`);
  }
}
module.exports = new LeadService();
