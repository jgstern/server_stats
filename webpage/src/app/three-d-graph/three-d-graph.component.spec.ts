import { ComponentFixture, TestBed } from '@angular/core/testing';

import { ThreeDGraphComponent } from './three-d-graph.component';

describe('ThreeDGraphComponent', () => {
  let component: ThreeDGraphComponent;
  let fixture: ComponentFixture<ThreeDGraphComponent>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      declarations: [ ThreeDGraphComponent ]
    })
    .compileComponents();
  });

  beforeEach(() => {
    fixture = TestBed.createComponent(ThreeDGraphComponent);
    component = fixture.componentInstance;
    fixture.detectChanges();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
