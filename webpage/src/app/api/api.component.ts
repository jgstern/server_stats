import { AfterViewInit, Component, ElementRef, OnDestroy, ViewEncapsulation, } from '@angular/core';

declare let Redoc: any

@Component({
  selector: 'app-api',
  templateUrl: './api.component.html',
  styleUrls: ['./api.component.scss'],
  encapsulation: ViewEncapsulation.None
})
export class ApiComponent implements AfterViewInit, OnDestroy {

  constructor(private element: ElementRef) { }

  ngAfterViewInit() {
    this.attachDocumentationComponent()
  }

  async attachDocumentationComponent() {
    const elem = this.element.nativeElement.querySelector('#redoc')

    Redoc.init('https://serverstats.nordgedanken.dev/assets/api_definition.yaml', {}, elem)
  }

  ngOnDestroy(): void {
    this.element.nativeElement.querySelector('#redoc').remove()
  }

}
