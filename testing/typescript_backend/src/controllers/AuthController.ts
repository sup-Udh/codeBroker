import { Request, Response } from 'express';
import { AuthService } from '../services/AuthService';
import { isValidEmail, isStrongPassword, validateRequiredFields } from '../utils/validator';

export class AuthController {
  private authService: AuthService;

  constructor(authService: AuthService) {
    this.authService = authService;
  }

  public register = async (req: Request, res: Response): Promise<void> => {
    try {
      const { email, password, name } = req.body;
      
      const missing = validateRequiredFields(req.body, ['email', 'password', 'name']);
      if (missing.length > 0) {
        res.status(400).json({ error: `Missing required fields: ${missing.join(', ')}` });
        return;
      }

      if (!isValidEmail(email)) {
        res.status(400).json({ error: 'Invalid email format' });
        return;
      }

      if (!isStrongPassword(password)) {
        res.status(400).json({ error: 'Password must be at least 8 characters long' });
        return;
      }

      const result = await this.authService.register(email, password, name);
      res.status(201).json(result);
    } catch (error: any) {
      if (error.message === 'User already exists') {
        res.status(409).json({ error: error.message });
      } else {
        res.status(500).json({ error: 'Internal server error' });
      }
    }
  };

  public login = async (req: Request, res: Response): Promise<void> => {
    try {
      const { email, password } = req.body;

      const missing = validateRequiredFields(req.body, ['email', 'password']);
      if (missing.length > 0) {
        res.status(400).json({ error: `Missing required fields: ${missing.join(', ')}` });
        return;
      }

      const result = await this.authService.login(email, password);
      res.status(200).json(result);
    } catch (error: any) {
      if (error.message === 'Invalid credentials') {
        res.status(401).json({ error: error.message });
      } else {
        res.status(500).json({ error: 'Internal server error' });
      }
    }
  };
}
