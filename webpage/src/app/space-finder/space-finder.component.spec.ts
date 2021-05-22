import { ComponentFixture, TestBed } from '@angular/core/testing';

import { SpaceFinderComponent } from './space-finder.component';

describe('SpaceFinderComponent', () => {
  let component: SpaceFinderComponent;
  let fixture: ComponentFixture<SpaceFinderComponent>;

  beforeEach(async () => {
    await TestBed.configureTestingModule({
      declarations: [ SpaceFinderComponent ]
    })
    .compileComponents();
  });

  beforeEach(() => {
    fixture = TestBed.createComponent(SpaceFinderComponent);
    component = fixture.componentInstance;
    fixture.detectChanges();
  });

  it('should create', () => {
    expect(component).toBeTruthy();
  });
});
