import { TestBed } from '@angular/core/testing';

import { GraphWebsocketService } from './graph-websocket.service';

describe('GraphWebsocketService', () => {
  let service: GraphWebsocketService;

  beforeEach(() => {
    TestBed.configureTestingModule({});
    service = TestBed.inject(GraphWebsocketService);
  });

  it('should be created', () => {
    expect(service).toBeTruthy();
  });
});
