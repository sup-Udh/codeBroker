import { Request, Response } from 'express';
import { UserService } from '../services/UserService';

export class UserController {
  private userService: UserService;

  constructor(userService: UserService) {
    this.userService = userService;
  }

  public getAllUsers = async (req: Request, res: Response): Promise<void> => {
    try {
      const users = await this.userService.getAllUsers();
      // Filter out password hashes
      const safeUsers = users.map(({ passwordHash, ...user }) => user);
      res.status(200).json(safeUsers);
    } catch (error) {
      res.status(500).json({ error: 'Internal server error' });
    }
  };

  public getUserById = async (req: Request, res: Response): Promise<void> => {
    try {
      const id = req.params.id;
      
      // Allow users to get their own profile or admin to get any
      if (req.user?.role !== 'admin' && req.user?.userId !== id) {
        res.status(403).json({ error: 'Forbidden' });
        return;
      }

      const user = await this.userService.getUserById(id);
      const { passwordHash, ...safeUser } = user;
      res.status(200).json(safeUser);
    } catch (error: any) {
      if (error.message === 'User not found') {
        res.status(404).json({ error: error.message });
      } else {
        res.status(500).json({ error: 'Internal server error' });
      }
    }
  };

  public getProfile = async (req: Request, res: Response): Promise<void> => {
    try {
      const id = req.user!.userId;
      const user = await this.userService.getUserById(id);
      const { passwordHash, ...safeUser } = user;
      res.status(200).json(safeUser);
    } catch (error) {
      res.status(500).json({ error: 'Internal server error' });
    }
  };
}
