const logger = require('./logger');

class CrmError extends Error {
  constructor(message, code) {
    super(message);
    this.code = code;
    logger.error(`CrmError created: ${message} (Code: ${code})`);
  }
}
module.exports = CrmError;
